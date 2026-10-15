//! The model oracle binds one loaded user message to the actual installed Skill.
use serde_json::Value;

const HEADER: &str = "Base directory for this skill: ";

fn loaded_candidate(
    input: &[Value],
    expected_body: &str,
    mut admitted_directory: impl FnMut(&str) -> bool,
) -> bool {
    input
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
                && admitted_directory(directory)
        })
}

#[cfg(windows)]
use hiroute_domain::CanonicalDigest;
#[cfg(windows)]
use hiroute_platform::{file_security, windows::security};
#[cfg(windows)]
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
#[cfg(windows)]
use zeroize::Zeroizing;

#[cfg(windows)]
const BOUND: u64 = 1_048_576; // Existing native settings/Skill snapshot bound.

#[cfg(windows)]
pub(super) struct Binding {
    original: PathBuf,
    canonical: PathBuf,
    directory: File,
    directory_id: (u32, u64),
    directory_permissions: file_security::WindowsPermissions,
    lease: security::ReadLease,
    file_id: (u32, u64),
    file_permissions: file_security::WindowsPermissions,
    digest: CanonicalDigest,
    body: Zeroizing<String>,
}

#[cfg(windows)]
fn rejected() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "native model Skill binding",
    )
}

#[cfg(windows)]
fn contents(file: &File) -> io::Result<Zeroizing<Vec<u8>>> {
    let mut file = file.try_clone()?;
    file.seek(SeekFrom::Start(0))?;
    let before = file.metadata()?;
    let mut bytes = Zeroizing::new(Vec::new());
    Read::by_ref(&mut file)
        .take(BOUND + 1)
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() as u64 > BOUND
        || before.len() != after.len()
        || bytes.len() as u64 != after.len()
        || before.modified()? != after.modified()?
    {
        return Err(rejected());
    }
    Ok(bytes)
}

#[cfg(windows)]
impl Binding {
    pub(super) fn retain(path: &Path) -> io::Result<Self> {
        if path.file_name() != Some(OsStr::new("SKILL.md")) {
            return Err(rejected());
        }
        let original = path.parent().ok_or_else(rejected)?.to_owned();
        super::native_paths::same_directory_spelling(&original, &original)?;
        let lease = security::open_read_lease(path)?;
        // Keep the former Snapshot::read private-file admission unchanged.
        security::verify_owner_only_handle(lease.file(), false)?;
        let directory = security::open_identity(&original, true)?;
        let directory_id = security::file_identity(&directory)?;
        let directory_permissions = file_security::capture(&directory)?;
        let canonical = std::fs::canonicalize(&original)?;
        let canonical_file = security::open_identity(&canonical, true)?;
        super::native_paths::same_directory_spelling(&canonical, &canonical)?;
        if security::file_identity(&canonical_file)? != directory_id
            || file_security::capture(&canonical_file)? != directory_permissions
        {
            return Err(rejected());
        }
        let bytes = contents(lease.file())?;
        let text = std::str::from_utf8(&bytes).map_err(|_| rejected())?;
        let body = text
            .strip_prefix("---\n")
            .and_then(|tail| tail.split_once("\n---\n"))
            .map(|(_, body)| body.trim())
            .filter(|body| !body.is_empty())
            .ok_or_else(rejected)?;
        let binding = Self {
            original,
            canonical,
            directory,
            directory_id,
            directory_permissions,
            file_id: security::file_identity(lease.file())?,
            file_permissions: file_security::capture(lease.file())?,
            digest: CanonicalDigest::of_bytes(&bytes),
            body: Zeroizing::new(body.to_owned()),
            lease,
        };
        binding.verify()?;
        Ok(binding)
    }

    pub(super) fn verify(&self) -> io::Result<()> {
        self.lease.verify()?;
        let current = security::open_identity(&self.original, true)?;
        let reopened = self.lease.reopen_file()?;
        if security::file_identity(&self.directory)? != self.directory_id
            || security::file_identity(&current)? != self.directory_id
            || file_security::capture(&self.directory)? != self.directory_permissions
            || file_security::capture(&current)? != self.directory_permissions
        {
            return Err(rejected());
        }
        for file in [self.lease.file(), &reopened] {
            security::verify_owner_only_handle(file, false)?;
            if security::file_identity(file)? != self.file_id
                || security::file_link_count(file)? != 1
                || file_security::capture(file)? != self.file_permissions
                || CanonicalDigest::of_bytes(&contents(file)?) != self.digest
            {
                return Err(rejected());
            }
        }
        Ok(())
    }

    fn directory_matches(&self, text: &str) -> bool {
        if text.encode_utf16().count() > 32767 {
            return false;
        }
        let observed = Path::new(text);
        let admitted = [self.original.as_path(), self.canonical.as_path()]
            .iter()
            .any(|expected| {
                super::native_paths::same_directory_spelling(observed, expected).unwrap_or(false)
            });
        admitted
            && security::open_identity(observed, true)
                .and_then(|file| {
                    Ok(security::file_identity(&file)? == self.directory_id
                        && file_security::capture(&file)? == self.directory_permissions)
                })
                .unwrap_or(false)
    }

    pub(super) fn matches(&self, input: &[Value]) -> bool {
        self.verify().is_ok()
            && loaded_candidate(input, &self.body, |directory| {
                self.directory_matches(directory)
            })
            && self.verify().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(text: String) -> Vec<Value> {
        vec![json!({"type":"message","role":"user","content":[{"type":"input_text","text":text}]})]
    }

    #[test]
    fn ordinary_qoder_skill_requires_one_exact_array_body_and_admitted_directory() {
        let matched =
            |items: &[Value]| loaded_candidate(items, "exact\nbody", |path| path == "same");
        assert!(matched(&input(format!("{HEADER}same\n\nexact\nbody"))));
        for text in [
            format!("wrapper\n{HEADER}same\n\nexact\nbody"),
            format!("{HEADER}same\r\n\r\nexact\nbody"),
            format!("{HEADER}same\n\nexact\r\nbody"),
            format!("{HEADER}same\n\nwrong body"),
            format!("{HEADER}foreign\n\nexact\nbody"),
        ] {
            assert!(!matched(&input(text)));
        }
        let mut separate = input(format!("{HEADER}foreign\n\nexact\nbody"));
        separate.extend(input(format!("{HEADER}same\n\nwrong body")));
        assert!(!matched(&separate));
        let mut wrong_shape = input(format!("{HEADER}same\n\nexact\nbody"));
        wrong_shape[0]["content"] = json!(format!("{HEADER}same\n\nexact\nbody"));
        assert!(!matched(&wrong_shape));
        wrong_shape = input(format!("{HEADER}same\n\nexact\nbody"));
        wrong_shape[0]["role"] = json!("assistant");
        assert!(!matched(&wrong_shape));
    }

    #[cfg(windows)]
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let outer = tempfile::tempdir().unwrap();
        let directory = outer.path().join("Skill 中文 ' ! &");
        security::create_owner_only_directory(&directory).unwrap();
        let path = directory.join("SKILL.md");
        hiroute_platform::durable::write_private_new(
            &path,
            b"---\nname: fixture\n---\n\nexact\nbody\n",
        )
        .unwrap();
        (outer, path)
    }

    #[cfg(windows)]
    #[test]
    fn ordinary_qoder_skill_native_binding_accepts_proven_drive_spelling_and_releases_restore() {
        let (_outer, path) = fixture();
        let binding = Binding::retain(&path).unwrap();
        let original = path.parent().unwrap().to_str().unwrap();
        let canonical = std::fs::canonicalize(path.parent().unwrap()).unwrap();
        assert!(original != canonical.to_str().unwrap());
        let text = format!("{HEADER}{original}\n\nexact\nbody");
        let old_prefix = format!("{HEADER}{}\n\n", canonical.display());
        assert!(
            !text.starts_with(&old_prefix),
            "the old literal verbatim oracle must reject"
        );
        assert!(binding.matches(&input(text)));
        assert!(binding.matches(&input(format!(
            "{HEADER}{}\n\nexact\nbody",
            canonical.display()
        ))));
        let file_id = security::file_identity(binding.lease.file()).unwrap();
        let sd = file_security::capture(binding.lease.file()).unwrap();
        assert!(
            security::open_read_write(&path).is_err(),
            "live data writers are denied"
        );
        assert!(
            std::fs::remove_file(&path).is_err(),
            "live restore cannot replace the retained Skill"
        );
        binding.verify().unwrap();
        drop(binding);
        assert!(
            security::file_identity(&security::open_identity(&path, false).unwrap()).unwrap()
                == file_id
        );
        assert!(
            file_security::capture(&security::open_identity(&path, false).unwrap()).unwrap() == sd
        );
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn ordinary_qoder_skill_native_binding_rejects_foreign_reparse_links_and_changed_permissions() {
        let (outer, path) = fixture();
        let binding = Binding::retain(&path).unwrap();
        let other = outer.path().join("foreign");
        security::create_owner_only_directory(&other).unwrap();
        let alias = other.join("alias");
        hiroute_platform_test_support::junction::create(&alias, path.parent().unwrap()).unwrap();
        assert!(Binding::retain(&alias.join("SKILL.md")).is_err());
        for directory in [
            other.to_str().unwrap(),
            alias.to_str().unwrap(),
            r"\\server\share\skill",
            r"\\.\C:\skill",
            r"C:\skill:stream",
        ] {
            assert!(!binding.matches(&input(format!("{HEADER}{directory}\n\nexact\nbody"))));
        }
        let before = security::file_identity(binding.lease.file()).unwrap();
        hiroute_platform_test_support::grant_everyone_read(&path, false).unwrap();
        assert!(security::file_identity(binding.lease.file()).unwrap() == before);
        assert!(binding.verify().is_err());
        drop(binding);
        std::fs::remove_dir(alias).unwrap();
        security::set_owner_only(&path, false).unwrap();
        let linked = other.join("linked-SKILL.md");
        std::fs::hard_link(&path, &linked).unwrap();
        assert!(Binding::retain(&path).is_err());
        std::fs::remove_file(linked).unwrap();
        assert_eq!(
            std::fs::read(path).unwrap(),
            b"---\nname: fixture\n---\n\nexact\nbody\n"
        );
    }

    #[cfg(windows)]
    #[test]
    fn ordinary_qoder_bash_cli_preserves_stable_hardlink_identity_and_literal_spelling() {
        use super::super::native_paths::BashFile;
        let outer = tempfile::tempdir().unwrap();
        let directory = outer.path().join("CLI 中文 ' ! &");
        security::create_owner_only_directory(&directory).unwrap();
        let image = directory.join("controlled-image.bin");
        let stable = directory.join("stable-image.bin");
        hiroute_platform::durable::write_private_new(
            &image,
            b"path projection unit control; never executed",
        )
        .unwrap();
        std::fs::hard_link(&image, &stable).unwrap();
        let original = security::open_identity(&stable, false).unwrap();
        let id = security::file_identity(&original).unwrap();
        let sd = file_security::capture(&original).unwrap();
        assert!(security::file_link_count(&original).unwrap() == 2);
        assert!(
            BashFile::retain(&stable).is_err(),
            "private data still requires one link"
        );
        let held = BashFile::trusted_cli(&stable).unwrap();
        let canonical = std::fs::canonicalize(&stable).unwrap();
        let other = BashFile::trusted_cli(&canonical).unwrap();
        assert!(held.quoted() == other.quoted());
        assert!(held.quoted().contains("'\\''") && !held.quoted().contains(r"\\?\"));
        assert!(security::open_read_write(&stable).is_err());
        assert!(std::fs::remove_file(&stable).is_err());
        held.verify().unwrap();
        other.verify().unwrap();
        assert!(security::file_identity(&original).unwrap() == id);
        assert!(file_security::capture(&original).unwrap() == sd);
        drop((held, other, original));
        std::fs::remove_file(stable).unwrap();
        assert!(std::fs::read(image).unwrap() == b"path projection unit control; never executed");
    }
}
