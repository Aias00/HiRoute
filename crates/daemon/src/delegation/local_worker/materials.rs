use hiroute_domain::delegation::DelegationErrorV1 as Error;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use crate::delegation::profile::RunMaterials;
use hiroute_platform::private_fs::{self, FileMode, OwnedDirectory};
pub(super) struct OwnedRoot {
    directory: OwnedDirectory,
    cleanup_on_drop: bool,
}

fn invalid<T>() -> Result<T, Error> {
    Err(Error::InvalidArguments)
}

pub(super) fn executable(path: &Path) -> Result<(), Error> {
    if !path.is_absolute() {
        return invalid();
    }
    let meta = fs::metadata(path).map_err(|_| Error::CapabilityUnavailable)?;
    if !meta.is_file() {
        return Err(Error::CapabilityUnavailable);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return Err(Error::CapabilityUnavailable);
        }
    }
    Ok(())
}

fn relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|c| match c {
            Component::Normal(name) => name
                .to_str()
                .is_some_and(|s| !s.contains(['\\', ':', '\0', '\r', '\n'])),
            _ => false,
        })
}

fn directory(path: &Path) -> Result<(), Error> {
    private_fs::create_directory(path).map_err(|_| Error::StorageUnavailable)
}

pub(super) fn prepare(
    materials: RunMaterials,
    root: PathBuf,
    session_root: PathBuf,
) -> Result<OwnedRoot, Error> {
    if !root.is_absolute()
        || materials.directories.len() + materials.files.len() > 256
        || materials
            .files
            .iter()
            .map(|f| f.contents.len())
            .sum::<usize>()
            > 1024 * 1024
    {
        return invalid();
    }
    // Canonicalize the existing parent, never create arbitrary ancestor directories.
    let parent = root.parent().ok_or(Error::InvalidArguments)?;
    let name = root.file_name().ok_or(Error::InvalidArguments)?;
    if root.components().any(|c| matches!(c, Component::ParentDir)) {
        return invalid();
    }
    let parent = fs::canonicalize(parent).map_err(|_| Error::InvalidArguments)?;
    let actual_root = parent.join(name);
    {
        let session = &session_root;
        if !session.is_absolute() {
            return invalid();
        }
        let session = fs::canonicalize(session).map_err(|_| Error::InvalidArguments)?;
        if !session.is_dir()
            || session.starts_with(&actual_root)
            || actual_root.starts_with(&session)
        {
            return invalid();
        }
    }
    let mut names = BTreeSet::new();
    for path in materials
        .directories
        .iter()
        .chain(materials.files.iter().map(|f| &f.relative_path))
    {
        if !relative(path) || !names.insert(path.clone()) {
            return invalid();
        }
    }
    // File parents must be explicitly listed. No implicit HOME/session directories.
    let dirs: BTreeSet<_> = materials.directories.iter().cloned().collect();
    for path in &names {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
            && !dirs.contains(parent)
        {
            return invalid();
        }
    }
    // An existing root is never ours to delete.
    let owned = OwnedRoot {
        directory: OwnedDirectory::create(&actual_root).map_err(|_| Error::StorageUnavailable)?,
        cleanup_on_drop: true,
    };
    let mut dirs: Vec<_> = materials.directories.into_iter().collect();
    dirs.sort_by_key(|p| p.components().count());
    for dir in dirs {
        directory(&owned.directory.path().join(dir))?;
    }
    for file in materials.files {
        let mode = if file.executable {
            FileMode::Executable
        } else {
            FileMode::Data
        };
        private_fs::write_material(
            &owned.directory.path().join(file.relative_path),
            &file.contents,
            mode,
        )
        .map_err(|_| Error::StorageUnavailable)?;
    }

    Ok(owned)
}

impl OwnedRoot {
    /// Active processes keep materials. Drop must not recursively remove their live root.
    pub(super) fn retain(&mut self) {
        self.cleanup_on_drop = false;
    }

    pub(super) fn cleanup(&mut self) -> Result<(), Error> {
        self.directory
            .remove()
            .map_err(|_| Error::StorageUnavailable)?;
        self.cleanup_on_drop = false;
        Ok(())
    }
}

impl Drop for OwnedRoot {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let _ = self.cleanup();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn windows_materials_pin_root_and_cleanup_only_owned_children() {
        use crate::delegation::profile::RunMaterialFile;
        let fixture = tempfile::tempdir().unwrap();
        let session = fixture.path().join("session");
        fs::create_dir(&session).unwrap();
        fs::write(session.join("history"), b"keep native history").unwrap();
        let path = fixture.path().join("run % ! & ' 空格");
        let mut owned = prepare(
            RunMaterials {
                directories: vec!["nested".into()],
                files: vec![RunMaterialFile {
                    relative_path: "nested/route.json".into(),
                    contents: zeroize::Zeroizing::new(b"public route fixture".to_vec()),
                    executable: false,
                }],
            },
            path.clone(),
            session.clone(),
        )
        .unwrap();
        hiroute_platform::windows::security::verify_owner_only(&path, true).unwrap();
        hiroute_platform::windows::security::verify_owner_only(
            &path.join("nested/route.json"),
            false,
        )
        .unwrap();
        assert!(fs::rename(&path, fixture.path().join("redirected")).is_err());
        owned.cleanup().unwrap();
        assert!(!path.exists());
        assert_eq!(
            fs::read(session.join("history")).unwrap(),
            b"keep native history"
        );
        owned.cleanup().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_materials_refuse_an_existing_unowned_root() {
        let fixture = tempfile::tempdir().unwrap();
        let session = fixture.path().join("session");
        fs::create_dir(&session).unwrap();
        let path = fixture.path().join("existing");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("foreign"), b"keep").unwrap();
        assert!(
            prepare(
                RunMaterials {
                    directories: vec![],
                    files: vec![]
                },
                path.clone(),
                session,
            )
            .is_err()
        );
        assert_eq!(fs::read(path.join("foreign")).unwrap(), b"keep");
    }

    #[tokio::test]
    async fn local_worker_cancelled_preparation_keeps_cleanup_owner() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cancelled");
        let copy = path.clone();
        let session = temp.path().join("session");
        fs::create_dir(&session).unwrap();
        let (created_tx, created_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let job = tokio::task::spawn_blocking(move || {
            let owned = prepare(
                RunMaterials {
                    directories: vec![],
                    files: vec![],
                },
                copy,
                session,
            )
            .unwrap();
            created_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            owned
        });
        created_rx.await.unwrap();
        assert!(path.exists());
        // Cancel the awaiter after creation while the blocking job still owns the root.
        drop(job);
        release_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while path.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}
