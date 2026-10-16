//! Retained Windows Skill authority and test-only closed receipt diagnostics.
//! Native paths, message bodies and permission descriptors stay in RAM.
#[cfg(test)]
use serde::Serialize;
use serde_json::Value;

const HEADER: &str = "Base directory for this skill: ";

#[cfg(test)]
#[derive(Debug, Serialize)]
pub(super) struct Facts {
    shape: &'static str,
    header_start: bool,
    header_embedded: bool,
    header_lf: bool,
    header_crlf: bool,
    exact_path: bool,
    path_kind: &'static str,
    path_identity: &'static str,
    expected_binding: &'static str,
    body_exact: bool,
    body_crlf_equivalent: bool,
    candidate_relation: &'static str,
}

#[cfg(test)]
impl Default for Facts {
    fn default() -> Self {
        Self {
            shape: "none",
            header_start: false,
            header_embedded: false,
            header_lf: false,
            header_crlf: false,
            exact_path: false,
            path_kind: "none",
            path_identity: "not_checked",
            expected_binding: "not_available",
            body_exact: false,
            body_crlf_equivalent: false,
            candidate_relation: "none",
        }
    }
}

#[cfg(test)]
impl Facts {
    pub(super) fn emit(self) {
        eprintln!(
            "native Qoder acceptance stage=skill-message diagnostic={}",
            serde_json::to_string(&self).expect("closed Skill diagnostic is serializable")
        );
    }
}

#[cfg(test)]
fn merge(current: &mut &'static str, value: &'static str, empty: &str) {
    if *current == empty {
        *current = value;
    } else if *current != value {
        *current = "mixed";
    }
}

#[cfg(test)]
pub(super) fn observe(
    input: &[Value],
    expected_directory: &str,
    expected_body: &str,
    #[cfg(windows)] mut native: Option<&mut NativeSkill>,
) -> Facts {
    let mut facts = Facts::default();
    #[cfg(windows)]
    let binding_before = native.as_mut().map(|native| native.binding());
    for item in input
        .iter()
        .filter(|item| item["type"] == "message" && item["role"] == "user")
    {
        match &item["content"] {
            Value::Array(parts) => {
                merge(&mut facts.shape, "array", "none");
                for part in parts {
                    if let Some(text) = part["text"].as_str() {
                        inspect_text(
                            &mut facts,
                            text,
                            expected_directory,
                            expected_body,
                            #[cfg(windows)]
                            native.as_deref(),
                        );
                    }
                }
            }
            Value::String(text) => {
                merge(&mut facts.shape, "string", "none");
                // Observe a different public input shape without making it
                // admissible to the unchanged array-only receipt matcher.
                inspect_text(
                    &mut facts,
                    text,
                    expected_directory,
                    expected_body,
                    #[cfg(windows)]
                    native.as_deref(),
                );
            }
            _ => merge(&mut facts.shape, "other", "none"),
        }
    }
    #[cfg(windows)]
    if let Some(native) = native {
        let after = native.binding();
        facts.expected_binding = match (binding_before, after) {
            (Some("unchanged"), "unchanged") => "unchanged",
            (Some("changed"), _) | (_, "changed") => "changed",
            _ => "query_failed",
        };
    }
    facts
}

#[cfg(test)]
fn inspect_text(
    facts: &mut Facts,
    text: &str,
    expected_directory: &str,
    expected_body: &str,
    #[cfg(windows)] native: Option<&NativeSkill>,
) {
    facts.header_embedded |= !text.starts_with(HEADER) && text.contains(HEADER);
    let Some(text) = text.strip_prefix(HEADER) else {
        return;
    };
    facts.header_start = true;
    let Some((line, remaining)) = text.split_once('\n') else {
        return;
    };
    let (directory, body) = if let Some(directory) = line.strip_suffix('\r') {
        let Some(body) = remaining.strip_prefix("\r\n") else {
            return;
        };
        facts.header_crlf = true;
        (directory, body)
    } else {
        let Some(body) = remaining.strip_prefix('\n') else {
            return;
        };
        facts.header_lf = true;
        (line, body)
    };
    let exact_path = directory == expected_directory;
    let body_exact = body.trim() == expected_body.trim();
    facts.exact_path |= exact_path;
    facts.body_exact |= body_exact;
    facts.body_crlf_equivalent |=
        body.trim().replace("\r\n", "\n") == expected_body.trim().replace("\r\n", "\n");
    #[cfg(windows)]
    let identity = {
        merge(&mut facts.path_kind, path_kind(directory), "none");
        let identity = native.map_or("not_checked", |native| native.directory_identity(directory));
        merge(&mut facts.path_identity, identity, "not_checked");
        identity
    };
    #[cfg(not(windows))]
    let identity = "not_checked";
    // Dimensions from different messages must not imply one correlated match.
    let relation = if exact_path && body_exact {
        "exact"
    } else if identity == "same" && body_exact {
        "same_native_directory"
    } else if body_exact {
        "body_only"
    } else if exact_path || identity == "same" {
        "path_only"
    } else {
        "neither"
    };
    merge(&mut facts.candidate_relation, relation, "none");
}

#[cfg(windows)]
use hiroute_platform::{file_security, windows::security};
#[cfg(windows)]
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf, Prefix},
};

#[cfg(windows)]
const SKILL_BOUND: u64 = 128 * 1024;

#[cfg(windows)]
fn drive_parts(path: &Path) -> io::Result<(u8, Vec<&OsStr>)> {
    let mut parts = path.components();
    let drive = match parts.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) if drive.is_ascii_alphabetic() => {
                drive
            }
            _ => return Err(io::ErrorKind::InvalidInput.into()),
        },
        _ => return Err(io::ErrorKind::InvalidInput.into()),
    };
    if parts.next() != Some(Component::RootDir) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut names = Vec::new();
    for part in parts {
        let Component::Normal(name) = part else {
            return Err(io::ErrorKind::InvalidInput.into());
        };
        if name
            .as_encoded_bytes()
            .iter()
            .any(|byte| matches!(byte, 0 | b':' | b'\r' | b'\n'))
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        names.push(name);
    }
    if names.is_empty() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok((drive, names))
}

#[cfg(windows)]
fn path_kind(text: &str) -> &'static str {
    if text.encode_utf16().count() > 32767 || drive_parts(Path::new(text)).is_err() {
        return "rejected";
    }
    match Path::new(text).components().next() {
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::VerbatimDisk(_)) => {
            "verbatim_disk"
        }
        _ => "disk",
    }
}

#[cfg(windows)]
fn same_directory_spelling(left: &Path, right: &Path) -> io::Result<bool> {
    let (drive, names) = drive_parts(left)?;
    let (expected_drive, expected_names) = drive_parts(right)?;
    Ok(drive.eq_ignore_ascii_case(&expected_drive)
        && names.len() == expected_names.len()
        && names.iter().zip(expected_names).all(|(name, expected)| {
            name.as_encoded_bytes()
                .eq_ignore_ascii_case(expected.as_encoded_bytes())
        }))
}

#[cfg(windows)]
fn admitted_directory_spelling(
    observed: &Path,
    original: &Path,
    canonical: &Path,
) -> io::Result<bool> {
    // Both expected names have already been bound to the retained native object.
    // This does not discover or follow an arbitrary observed alias.
    Ok(same_directory_spelling(observed, original)?
        || same_directory_spelling(observed, canonical)?)
}

#[cfg(windows)]
pub(super) struct NativeSkill {
    path: PathBuf,
    canonical_path: PathBuf,
    directory: File,
    file: File,
    directory_id: (u32, u64),
    file_id: (u32, u64),
    directory_permissions: file_security::WindowsPermissions,
    file_permissions: file_security::WindowsPermissions,
    content: hiroute_domain::CanonicalDigest,
    #[cfg(test)]
    _fixture_root: Option<tempfile::TempDir>,
}

#[cfg(windows)]
impl NativeSkill {
    pub(super) fn retain(directory: &Path, content: &[u8], private: bool) -> io::Result<Self> {
        drive_parts(directory)?;
        let directory_file = security::open_identity(directory, true)?;
        let mut file = security::open_identity_at(&directory_file, OsStr::new("SKILL.md"), false)?;
        if private {
            security::verify_owner_only_handle(&directory_file, true)?;
            security::verify_owner_only_handle(&file, false)?;
        }
        if security::file_link_count(&file)? != 1 || file.metadata()?.len() > SKILL_BOUND {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let expected = hiroute_domain::CanonicalDigest::of_bytes(content);
        if digest(&mut file)? != expected {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let directory_id = security::file_identity(&directory_file)?;
        let directory_permissions = file_security::capture(&directory_file)?;
        // canonicalize can change component spelling as well as the prefix.
        // Establish its authority only after the original no-reparse open, and
        // then reopen that name through the same native no-reparse boundary.
        let canonical_path = std::fs::canonicalize(directory)?;
        drive_parts(&canonical_path)?;
        let canonical_directory = security::open_identity(&canonical_path, true)?;
        if security::file_identity(&canonical_directory)? != directory_id
            || file_security::capture(&canonical_directory)? != directory_permissions
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(Self {
            path: directory.to_owned(),
            canonical_path,
            directory_id,
            file_id: security::file_identity(&file)?,
            directory_permissions,
            file_permissions: file_security::capture(&file)?,
            directory: directory_file,
            file,
            content: expected,
            #[cfg(test)]
            _fixture_root: None,
        })
    }

    /// Only one actual user-array text may supply both the directory and body.
    /// Recheck the held file and full descriptors on both sides of the query.
    pub(super) fn matches_loaded_user(&mut self, input: &[Value], expected_body: &str) -> bool {
        if self.binding() != "unchanged" {
            return false;
        }
        let matched = input
            .iter()
            .filter(|item| item["type"] == "message" && item["role"] == "user")
            .filter_map(|item| item["content"].as_array())
            .flatten()
            .filter_map(|part| part["text"].as_str())
            .any(|text| {
                let Some(text) = text.strip_prefix(HEADER) else {
                    return false;
                };
                let Some((directory, body)) = text.split_once("\n\n") else {
                    return false;
                };
                !directory.contains(['\r', '\n'])
                    && body.trim() == expected_body.trim()
                    && self.directory_identity(directory) == "same"
            });
        self.binding() == "unchanged" && matched
    }

    #[cfg(test)]
    pub(super) fn keep_fixture_root(&mut self, root: tempfile::TempDir) {
        self._fixture_root = Some(root);
    }

    fn binding(&mut self) -> &'static str {
        let checked = (|| -> io::Result<bool> {
            let current = security::open_identity(&self.path, true)?;
            let skill = security::open_identity_at(&self.directory, OsStr::new("SKILL.md"), false)?;
            Ok(security::file_identity(&current)? == self.directory_id
                && security::file_identity(&self.directory)? == self.directory_id
                && security::file_identity(&self.file)? == self.file_id
                && security::file_identity(&skill)? == self.file_id
                && security::file_link_count(&skill)? == 1
                && file_security::capture(&self.directory)? == self.directory_permissions
                && file_security::capture(&current)? == self.directory_permissions
                && file_security::capture(&self.file)? == self.file_permissions
                && file_security::capture(&skill)? == self.file_permissions
                && digest(&mut self.file)? == self.content)
        })();
        match checked {
            Ok(true) => "unchanged",
            Ok(false) => "changed",
            Err(_) => "query_failed",
        }
    }

    fn directory_identity(&self, observed: &str) -> &'static str {
        let admitted = (|| -> io::Result<bool> {
            if path_kind(observed) == "rejected" {
                return Ok(false);
            }
            // This is a read boundary, not an identity decision: only the
            // original or proven canonical name may be queried. Kernel FileID
            // and the retained descriptor remain the authority after admission.
            admitted_directory_spelling(Path::new(observed), &self.path, &self.canonical_path)
        })();
        if !matches!(admitted, Ok(true)) {
            return "rejected";
        }
        match security::open_identity(Path::new(observed), true)
            .and_then(|file| security::file_identity(&file))
        {
            Ok(identity) if identity == self.directory_id => "same",
            Ok(_) => "different",
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => "rejected",
            Err(_) => "query_failed",
        }
    }
}

#[cfg(windows)]
fn digest(file: &mut File) -> io::Result<hiroute_domain::CanonicalDigest> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    file.take(SKILL_BOUND + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > SKILL_BOUND {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(hiroute_domain::CanonicalDigest::of_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(content: Value) -> Vec<Value> {
        vec![json!({"type":"message","role":"user","content":content})]
    }

    fn facts(content: Value) -> Facts {
        observe(
            &input(content),
            "expected-directory",
            "first\nsecond",
            #[cfg(windows)]
            None,
        )
    }

    #[test]
    fn skill_diagnostic_separates_shape_header_path_and_body_without_exposing_content() {
        let missing = facts(json!([{"type":"input_text","text":"private-value"}]));
        assert_eq!(missing.shape, "array");
        assert!(!missing.header_start && !missing.exact_path && !missing.body_exact);
        let wrong_path =
            facts(json!([{"text":format!("{HEADER}different-directory\n\nfirst\nsecond")}]));
        assert!(wrong_path.header_start && !wrong_path.exact_path && wrong_path.body_exact);
        assert_eq!(wrong_path.candidate_relation, "body_only");
        let wrong_body =
            facts(json!([{"text":format!("{HEADER}expected-directory\n\nprivate-value")}]));
        assert!(wrong_body.exact_path && !wrong_body.body_exact);
        assert_eq!(wrong_body.candidate_relation, "path_only");
        let encoded = serde_json::to_string(&wrong_body).unwrap();
        assert!(!encoded.contains("private-value") && !encoded.contains("expected-directory"));
        let different_shape = facts(json!(format!(
            "{HEADER}expected-directory\n\nfirst\nsecond"
        )));
        assert_eq!(different_shape.shape, "string");
        assert!(different_shape.exact_path && different_shape.body_exact);
        let separate = facts(json!([
            {"text":format!("{HEADER}different-directory\n\nfirst\nsecond")},
            {"text":format!("{HEADER}expected-directory\n\nprivate-value")}
        ]));
        assert!(separate.exact_path && separate.body_exact);
        assert_eq!(separate.candidate_relation, "mixed");
    }

    #[test]
    fn skill_diagnostic_separates_crlf_from_exact_body_and_rejects_missing_header_separator() {
        let crlf =
            facts(json!([{"text":format!("{HEADER}expected-directory\r\n\r\nfirst\r\nsecond")}]));
        assert!(crlf.header_crlf && !crlf.header_lf && crlf.exact_path);
        assert!(!crlf.body_exact && crlf.body_crlf_equivalent);
        let broken = facts(json!([{"text":format!("{HEADER}expected-directory\nfirst\nsecond")}]));
        assert!(broken.header_start && !broken.header_lf && !broken.header_crlf);
        assert!(!broken.exact_path && !broken.body_exact);
        let embedded = facts(
            json!([{"text":format!("wrapper\n{HEADER}expected-directory\n\nfirst\nsecond")}]),
        );
        assert!(embedded.header_embedded && !embedded.header_start && !embedded.body_exact);
        let later =
            facts(json!([{"text":format!("{HEADER}expected-directory\n\nfirst\r\n\r\nsecond")}]));
        assert!(later.header_lf && !later.header_crlf && later.exact_path);
    }

    #[test]
    fn skill_diagnostic_does_not_confuse_assistant_or_tool_output_with_loaded_user_text() {
        let observed = observe(
            &[
                json!({"type":"message","role":"assistant","content":[{"text":format!("{HEADER}expected-directory\n\nfirst\nsecond")}]}),
                json!({"type":"function_call_output","output":"private-value"}),
            ],
            "expected-directory",
            "first\nsecond",
            #[cfg(windows)]
            None,
        );
        assert_eq!(observed.shape, "none");
        assert!(!observed.header_start && !observed.exact_path && !observed.body_exact);
    }

    #[cfg(windows)]
    fn native() -> (
        super::super::super::probe_files::PrivateProbeRoot,
        NativeSkill,
    ) {
        use std::io::Write;
        let root =
            super::super::super::probe_files::temporary("hiroute-skill-diagnostic-").unwrap();
        let mut file = security::create_owner_only_file(&root.path().join("SKILL.md")).unwrap();
        file.write_all(b"same Skill bytes\n").unwrap();
        drop(file);
        let held = NativeSkill::retain(root.path(), b"same Skill bytes\n", true).unwrap();
        (root, held)
    }

    #[cfg(windows)]
    #[test]
    fn windows_skill_diagnostic_binds_drive_spellings_to_one_actual_directory_and_content() {
        let (root, mut held) = native();
        let extended = std::fs::canonicalize(root.path()).unwrap();
        assert_eq!(
            security::file_identity(&security::open_identity(&extended, true).unwrap()).unwrap(),
            held.directory_id,
            "the canonical name must open the original retained directory"
        );
        assert_eq!(
            held.directory_identity(root.path().to_str().unwrap()),
            "same"
        );
        assert_eq!(held.directory_identity(extended.to_str().unwrap()), "same");
        assert_eq!(held.binding(), "unchanged");
        let expected = extended.to_str().unwrap();
        // Only this known local-drive fixture spelling is changed. The next
        // query proves the projected name still opens the retained native FI.
        let ordinary = expected.strip_prefix(r"\\?\").unwrap();
        assert_eq!(held.directory_identity(ordinary), "same");
        assert_ne!(expected, ordinary);
        let text = format!("{HEADER}{ordinary}\n\nsame Skill bytes");
        assert!(!text.starts_with(&format!("{HEADER}{expected}\n\n")));
        let observed = observe(
            &input(json!([{"type":"input_text","text":text}])),
            expected,
            "same Skill bytes",
            Some(&mut held),
        );
        assert!(!observed.exact_path && observed.body_exact);
        assert_eq!(observed.path_identity, "same");
        assert_eq!(observed.expected_binding, "unchanged");
        assert_eq!(observed.candidate_relation, "same_native_directory");
        assert!(held.matches_loaded_user(
            &input(json!([{ "text":format!("{HEADER}{ordinary}\n\nsame Skill bytes") }])),
            "same Skill bytes"
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_skill_diagnostic_admits_only_original_or_proven_canonical_components() {
        let original = Path::new(r"C:\SHORT~1\private");
        let canonical = Path::new(r"\\?\C:\Long directory name\private");
        // A prefix projection alone cannot cover canonical component changes.
        // The previous original-only admission rejects this canonical name.
        assert!(!same_directory_spelling(canonical, original).unwrap());
        assert!(admitted_directory_spelling(canonical, original, canonical).unwrap());
        assert!(
            admitted_directory_spelling(
                Path::new(r"c:\Long directory name\PRIVATE"),
                original,
                canonical
            )
            .unwrap()
        );
        for rejected in [
            r"C:\Long directory name\other",
            r"C:\other\private",
            r"D:\Long directory name\private",
            r"C:\Long directory name\private:stream",
            r"C:\Long directory name\private\..\private",
            r"\\server\share\private",
            r"\\.\C:\Long directory name\private",
            r"relative\private",
        ] {
            assert!(
                !admitted_directory_spelling(Path::new(rejected), original, canonical)
                    .unwrap_or(false)
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_skill_diagnostic_rejects_foreign_reparse_device_and_ads_paths() {
        let (root, mut held) = native();
        let other = super::super::super::probe_files::temporary("hiroute-other-skill-").unwrap();
        let alias = other.path().join("alias");
        hiroute_platform_test_support::junction::create(&alias, root.path()).unwrap();
        assert!(NativeSkill::retain(&alias, b"same Skill bytes\n", true).is_err());
        for path in [
            alias.to_str().unwrap().to_owned(),
            other.path().to_str().unwrap().to_owned(),
            format!("{}:stream", root.path().display()),
            r"\\server\share\skill".into(),
            r"\\.\C:\skill".into(),
            "relative/skill".into(),
            format!("{}\\..\\skill", root.path().display()),
        ] {
            assert_eq!(held.directory_identity(&path), "rejected");
            assert!(!held.matches_loaded_user(
                &input(json!([{ "text":format!("{HEADER}{path}\n\nsame Skill bytes") }])),
                "same Skill bytes"
            ));
        }
        std::fs::remove_dir(&alias).unwrap();
        assert!(root.path().join("SKILL.md").is_file());
        drop(held);
        let link = other.path().join("linked-SKILL.md");
        std::fs::hard_link(root.path().join("SKILL.md"), &link).unwrap();
        assert!(NativeSkill::retain(root.path(), b"same Skill bytes\n", true).is_err());
        std::fs::remove_file(link).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_skill_diagnostic_detects_changed_content_on_the_same_retained_file() {
        use std::io::Write;
        let (root, mut held) = native();
        let identity = held.file_id;
        let mut writer = security::open_read_write(&root.path().join("SKILL.md")).unwrap();
        writer.seek(SeekFrom::Start(0)).unwrap();
        writer.write_all(b"changed Skill!\n").unwrap();
        writer.flush().unwrap();
        assert_eq!(security::file_identity(&writer).unwrap(), identity);
        assert_eq!(held.binding(), "changed");
        assert!(!held.matches_loaded_user(
            &input(
                json!([{ "text":format!("{HEADER}{}\n\nsame Skill bytes", root.path().display()) }])
            ),
            "same Skill bytes"
        ));
        // This controlled fixture is not a provider/account file. Restore only
        // its own still-held object, so cleanup retains the original authority.
        writer.seek(SeekFrom::Start(0)).unwrap();
        writer.set_len(0).unwrap();
        writer.write_all(b"same Skill bytes\n").unwrap();
        assert_eq!(held.binding(), "unchanged");
        hiroute_platform_test_support::grant_everyone_read(&root.path().join("SKILL.md"), false)
            .unwrap();
        assert_eq!(security::file_identity(&writer).unwrap(), identity);
        assert_eq!(held.binding(), "changed");
        assert!(!held.matches_loaded_user(
            &input(
                json!([{ "text":format!("{HEADER}{}\n\nsame Skill bytes", root.path().display()) }])
            ),
            "same Skill bytes"
        ));
        let mut borrowed = NativeSkill::retain(root.path(), b"same Skill bytes\n", false).unwrap();
        assert_eq!(borrowed.binding(), "unchanged");
        assert!(NativeSkill::retain(root.path(), b"same Skill bytes\n", true).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn windows_loaded_skill_never_combines_foreign_body_shape_or_policy_facts() {
        let (root, mut held) = native();
        let valid = format!("{HEADER}{}\n\nsame Skill bytes", root.path().display());
        let wrong_body = format!("{HEADER}{}\n\nwrong body", root.path().display());
        let foreign = format!("{HEADER}C:\\foreign\n\nsame Skill bytes");
        assert!(!held.matches_loaded_user(
            &input(json!([{ "text":wrong_body }, { "text":foreign }])),
            "same Skill bytes"
        ));
        assert!(!held.matches_loaded_user(&input(json!(valid.clone())), "same Skill bytes"));
        assert!(!held.matches_loaded_user(
            &[json!({"type":"message","role":"assistant","content":[{"text":valid}]})],
            "same Skill bytes"
        ));
        assert!(!held.matches_loaded_user(
            &input(json!([{ "text":valid.replace('\n', "\r\n") }])),
            "same Skill bytes"
        ));
        assert!(held.matches_loaded_user(&input(json!([{ "text":valid }])), "same Skill bytes"));
    }
}
