//! Hidden, synchronous administrator maintenance for an authenticated SCM owner.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Component, Path};

use hiroute_host_runtime::{StandaloneInstallRecordV1, standalone_daemon_binary};
use hiroute_platform::windows::background::{Definition, Manager};
use hiroute_platform::windows::scm_owner::{self, Installation, Operation, Owner};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::json;
use zeroize::Zeroizing;

pub(super) const VERB: &str = "__windows-scm-owner-install";
const SCHEMA: &str = "hiroute.windows-scm-owner-request/v1";
const MAX_REQUEST: u64 = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    operation: RequestedOperation,
    account: String,
    #[serde(deserialize_with = "secret")]
    password: Zeroizing<String>,
    enabled: Option<bool>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RequestedOperation {
    Install,
    Uninstall,
    SetAutostart,
}

fn secret<'de, D: Deserializer<'de>>(decoder: D) -> Result<Zeroizing<String>, D::Error> {
    String::deserialize(decoder).map(Zeroizing::new)
}

impl Request {
    fn operation(&self) -> io::Result<Operation> {
        if self.schema != SCHEMA
            || self.account.is_empty()
            || self.account.len() > 512
            || self.account.contains(['\0', '\n', '\r'])
            || !self.account.contains('\\')
            || self.password.is_empty()
            || self.password.len() > 4096
            || self.password.contains('\0')
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        match (self.operation, self.enabled) {
            (RequestedOperation::Install, None) => Ok(Operation::Install),
            (RequestedOperation::Uninstall, None) => Ok(Operation::Uninstall),
            (RequestedOperation::SetAutostart, Some(enabled)) => {
                Ok(Operation::SetAutostart(enabled))
            }
            _ => Err(io::ErrorKind::InvalidInput.into()),
        }
    }

    fn operation_name(&self) -> &'static str {
        match self.operation {
            RequestedOperation::Install => "install",
            RequestedOperation::Uninstall => "uninstall",
            RequestedOperation::SetAutostart => "set_autostart",
        }
    }
}

pub(super) fn run_hidden(arguments: &[OsString]) -> u8 {
    if arguments != [OsString::from("--request-stdin")] {
        let _ = io::stderr().write_all(b"hiroute Windows SCM owner request invalid\n");
        return 2;
    }
    match run(io::stdin().lock(), io::stdout().lock()) {
        Ok(()) => 0,
        Err(_) => {
            // Neither native errors, owner-controlled paths nor request/password bytes
            // are replayed to output. The administrator receives one fixed diagnosis.
            let _ = io::stderr().write_all(b"hiroute Windows SCM owner maintenance unavailable\n");
            6
        }
    }
}

fn run(input: impl Read, mut output: impl Write) -> io::Result<()> {
    let request = read_request(input)?;
    let operation = request.operation()?;
    let definition = scm_owner::execute(&request.account, &request.password, operation, |owner| {
        inspect_installation(owner, operation)
    })?;
    let result = json!({
        "schema": "hiroute.windows-scm-owner-result/v1",
        "operation": request.operation_name(),
        "definition": definition,
        "completed": true
    });
    let bytes = serde_json::to_vec(&result).map_err(io::Error::other)?;
    if bytes.len() > MAX_REQUEST as usize {
        return Err(io::ErrorKind::InvalidData.into());
    }
    output.write_all(&bytes)?;
    output.write_all(b"\n")?;
    output.flush()
}

fn read_request(input: impl Read) -> io::Result<Request> {
    let mut bytes = Zeroizing::new(Vec::new());
    input.take(MAX_REQUEST + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > MAX_REQUEST as usize {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let request: Request =
        serde_json::from_slice(&bytes).map_err(|_| io::ErrorKind::InvalidInput)?;
    request.operation()?;
    Ok(request)
}

fn inspect_installation(owner: &Owner, operation: Operation) -> io::Result<Installation> {
    let data = owner.profile.join(".local/share/hiroute");
    let marker = data.join("standalone.json");
    let definition_path = data.join("service/windows-background.json");
    let record: StandaloneInstallRecordV1 = read_private(owner, &marker)?;
    record.validate().map_err(|_| io::ErrorKind::InvalidData)?;
    if record.target != hiroute_host_runtime::standalone_host_target()
        || record.service_definition != definition_path
        || !is_versioned_root(&owner.profile, &record.install_root)
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let previous: Option<Definition> = match read_private(owner, &definition_path) {
        Ok(value) => Some(value),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if let Some(previous) = &previous {
        previous.validate_for_owner_sid(&owner.sid)?;
        if previous.manager != Manager::Scm {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
    }
    match operation {
        Operation::Install => {
            let daemon = standalone_daemon_binary(&record.install_root);
            let mut image = owner.open_image(&daemon)?;
            let definition =
                Definition::from_owner_image(Manager::Scm, daemon, &owner.sid, &mut image)?;
            Ok(Installation {
                definition,
                previous,
                image: Some(image),
            })
        }
        Operation::Uninstall | Operation::SetAutostart(_) => {
            // A damaged/missing bundle must not trap its machine registration. The
            // private owner definition and native SCM config still require an exact
            // authenticated SID/account/command match before deletion or policy change.
            let definition = previous.ok_or(io::ErrorKind::NotFound)?;
            if definition.daemon != standalone_daemon_binary(&record.install_root) {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            Ok(Installation {
                definition,
                previous: None,
                image: None,
            })
        }
    }
}

fn is_versioned_root(profile: &Path, root: &Path) -> bool {
    root.strip_prefix(profile.join(".local/lib/hiroute"))
        .is_ok_and(|relative| {
            let mut parts = relative.components();
            matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
        })
}

fn read_private<T: DeserializeOwned>(owner: &Owner, path: &Path) -> io::Result<T> {
    let file = owner.open_private_file(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_REQUEST + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > MAX_REQUEST as usize {
        return Err(io::ErrorKind::InvalidData.into());
    }
    serde_json::from_slice(&bytes).map_err(|_| io::ErrorKind::InvalidData.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> serde_json::Value {
        json!({"schema":SCHEMA,"operation":"install","account":"MACHINE\\standard-owner",
               "password":"synthetic_owner_stdin_only_password","enabled":null})
    }

    #[test]
    fn bounded_closed_request_never_accepts_sid_paths_or_extra_policy() {
        let value = request();
        assert_eq!(
            read_request(serde_json::to_vec(&value).unwrap().as_slice())
                .unwrap()
                .operation()
                .unwrap(),
            Operation::Install
        );
        for (key, extra) in [
            ("owner_sid", json!("S-1-5-18")),
            ("daemon", json!("C:\\Windows\\malicious.exe")),
            ("enabled", json!(true)),
            ("operation", json!("start")),
            ("account", json!("standard-owner")),
        ] {
            let mut invalid = value.clone();
            invalid[key] = extra;
            assert!(read_request(serde_json::to_vec(&invalid).unwrap().as_slice()).is_err());
        }
        assert!(read_request([b' '; MAX_REQUEST as usize + 1].as_slice()).is_err());
        let mut invalid = value;
        invalid["password"] = json!("x".repeat(4097));
        assert!(read_request(serde_json::to_vec(&invalid).unwrap().as_slice()).is_err());
    }

    #[test]
    fn policy_change_requires_one_explicit_boolean_and_versioned_owner_root() {
        let mut value = request();
        value["operation"] = json!("set_autostart");
        assert!(read_request(serde_json::to_vec(&value).unwrap().as_slice()).is_err());
        value["enabled"] = json!(false);
        assert_eq!(
            read_request(serde_json::to_vec(&value).unwrap().as_slice())
                .unwrap()
                .operation()
                .unwrap(),
            Operation::SetAutostart(false)
        );
        let profile = Path::new("C:\\Users\\standard-owner");
        assert!(is_versioned_root(
            profile,
            &profile.join(".local/lib/hiroute/0.3.1-revision")
        ));
        assert!(!is_versioned_root(
            profile,
            &profile.join(".local/lib/hiroute")
        ));
        assert!(!is_versioned_root(
            profile,
            &profile.join(".local/lib/hiroute/version/nested")
        ));
        assert!(!is_versioned_root(
            profile,
            Path::new("C:\\Users\\other-owner\\.local\\lib\\hiroute\\version")
        ));
    }
}
