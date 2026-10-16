//! Bounded observations of the borrowed native model target. Only the public
//! settings transaction writes/restores it; this leaf never reads auth material.
#[cfg(windows)]
use hiroute_platform::file_security::{self, WindowsPermissions};
#[cfg(windows)]
use hiroute_platform::windows::security;
use serde_json::Value;
#[cfg(windows)]
use std::io::Read;
#[cfg(windows)]
use std::path::Path;
use zeroize::Zeroizing;

#[cfg(windows)]
pub struct Snapshot {
    pub bytes: Zeroizing<Vec<u8>>,
    pub permissions: WindowsPermissions,
}
#[cfg(windows)]
impl Snapshot {
    /// Original native settings use the published readable-external-object
    /// contract. Capturing their actual permissions never adopts or edits them.
    pub fn external_optional(path: &Path) -> Option<Self> {
        Self::capture(path, false)
    }
    fn capture(path: &Path, private: bool) -> Option<Self> {
        let mut file = match security::open_identity(path, false) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(_) => panic!("native settings must retain readable reparse-free identity"),
        };
        if private {
            security::verify_owner_only_handle(&file, false)
                .expect("managed native settings must retain strict private ownership");
        }
        assert!(
            security::file_link_count(&file).unwrap() == 1,
            "native settings cannot have aliases"
        );
        let mut bytes = Zeroizing::new(Vec::new());
        let before = file.metadata().unwrap();
        let permissions = file_security::capture(&file).unwrap();
        Read::by_ref(&mut file)
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .unwrap();
        assert!(
            bytes.len() <= 1_048_576,
            "native settings exceed their production bound"
        );
        let after = file.metadata().unwrap();
        assert!(
            before.len() == after.len()
                && after.len() == bytes.len() as u64
                && before.modified().unwrap() == after.modified().unwrap()
                && security::file_link_count(&file).unwrap() == 1
                && permissions == file_security::capture(&file).unwrap(),
            "native settings changed during held snapshot capture"
        );
        Some(Self { bytes, permissions })
    }
    pub fn read(path: &Path) -> Self {
        Self::capture(path, true).expect("publicly configured native settings must exist")
    }
    pub fn value(&self) -> Value {
        jsonc(&self.bytes).expect("public native settings must retain valid JSONC")
    }
    pub fn same(&self, other: &Self) -> bool {
        *self.bytes == *other.bytes && self.permissions == other.permissions
    }
}

/// Observe untouched user data independently of the producer's renderer. This
/// parser is an oracle only: production admission still owns all JSONC rules.
fn jsonc(bytes: &[u8]) -> Result<Value, &'static str> {
    std::str::from_utf8(bytes).map_err(|_| "settings-encoding")?;
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let mut clean = Zeroizing::new(bytes.to_vec());
    let mut position = 0;
    let mut quoted = false;
    while position < bytes.len() {
        if quoted {
            match bytes[position] {
                b'\\' => position += 2,
                b'"' => {
                    quoted = false;
                    position += 1;
                }
                _ => position += 1,
            }
            continue;
        }
        if bytes[position] == b'"' {
            quoted = true;
            position += 1;
        } else if bytes.get(position..position + 2) == Some(b"//") {
            while position < bytes.len() && bytes[position] != b'\n' {
                clean[position] = b' ';
                position += 1;
            }
        } else if bytes.get(position..position + 2) == Some(b"/*") {
            clean[position..position + 2].fill(b' ');
            position += 2;
            loop {
                if bytes.get(position..position + 2) == Some(b"*/") {
                    clean[position..position + 2].fill(b' ');
                    position += 2;
                    break;
                }
                let byte = *bytes.get(position).ok_or("settings-comment")?;
                if byte != b'\n' && byte != b'\r' {
                    clean[position] = b' ';
                }
                position += 1;
            }
        } else {
            position += 1;
        }
    }
    if quoted {
        return Err("settings-string");
    }
    let value: Value = serde_json::from_slice(&clean).map_err(|_| "settings-json")?;
    if !value.is_object() {
        return Err("settings-root");
    }
    Ok(value)
}

pub fn without_owned(value: &Value, providers: &[String], original: Option<&Value>) -> Value {
    let mut result = value.clone();
    if let Some(entries) = result.get_mut("providers").and_then(Value::as_object_mut) {
        for provider in providers {
            entries.remove(provider);
        }
        if entries.is_empty() && original.and_then(|value| value.get("providers")).is_none() {
            result.as_object_mut().unwrap().remove("providers");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn ordinary_qoder_defaults_oracle_preserves_user_fields_and_jsonc_literals() {
        let original = jsonc(br#"{/* native default */"model":{"name":"user/model"},"url":"https://example.invalid/a//b","literal":"quoted \\\" /* text */","providers":{"user":{"keep":true}}}// trailing"#).unwrap();
        let mut configured = original.clone();
        configured["providers"]["owned/responses"] = json!({"apiKey":"test-only"});
        configured["providers"]["owned/messages"] = json!({"protocol":"anthropic"});
        let owned = ["owned/responses".to_owned(), "owned/messages".to_owned()];
        assert!(without_owned(&configured, &owned, Some(&original)) == original);
        configured["model"]["name"] = json!("owned/responses/model");
        assert!(without_owned(&configured, &owned, Some(&original)) != original);
        assert!(
            without_owned(&json!({"providers":{"owned/responses":{}}}), &owned, None) == json!({})
        );
        assert!(jsonc(b"{ /* unterminated").is_err());
        assert!(jsonc(b"[]").is_err());
        assert!(jsonc(b"{\"model\":\"unfinished}").is_err());
    }
}
