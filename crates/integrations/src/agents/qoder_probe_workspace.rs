//! Qoder and its Git Bash shell must receive the same ordinary drive spelling.
//! Rust's canonical verbatim path is not a portable shell working directory.
use hiroute_platform::windows::security;
use std::{
    fs::File,
    io,
    path::{Component, Path, PathBuf, Prefix},
};

pub(super) fn retain(path: &Path) -> io::Result<(PathBuf, File)> {
    let disk = |path: &Path| {
        matches!(
            path.components().next(),
            Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        )
    };
    if !path.is_absolute() || !disk(path) {
        return Err(invalid());
    }
    // Inspect the original private object before canonicalization could hide aliases.
    let held = security::open_owner_only(path, true)?;
    let canonical = std::fs::canonicalize(path)?;
    let mut components = canonical.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(invalid());
    };
    let drive = match prefix.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) if drive.is_ascii_alphabetic() => drive,
        _ => return Err(invalid()),
    };
    if components.next() != Some(Component::RootDir) {
        return Err(invalid());
    }
    let mut ordinary = PathBuf::from(format!("{}:\\", char::from(drive)));
    for part in components {
        let Component::Normal(name) = part else {
            return Err(invalid());
        };
        ordinary.push(name);
    }
    let roundtrip = security::open_owner_only(&ordinary, true)?;
    if std::fs::canonicalize(&ordinary)? != canonical
        || security::file_identity(&held)? != security::file_identity(&roundtrip)?
    {
        return Err(invalid());
    }
    Ok((ordinary, held))
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "native private workspace identity",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_workspace_uses_an_ordinary_name_for_the_same_private_directory() {
        let root = super::super::super::probe_files::temporary("hiroute-qoder-workspace-").unwrap();
        let directory = root.path().join("shell workspace 测试");
        drop(security::create_owner_only_directory(&directory).unwrap());
        let canonical = std::fs::canonicalize(&directory).unwrap();
        assert!(matches!(
            canonical.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::VerbatimDisk(_))
        ));
        for original in [&directory, &canonical] {
            let (ordinary, held) = retain(original).unwrap();
            assert!(matches!(
                ordinary.components().next(),
                Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_))
            ));
            assert_eq!(std::fs::canonicalize(&ordinary).unwrap(), canonical);
            let opened = security::open_owner_only(&directory, true).unwrap();
            assert_eq!(
                security::file_identity(&held).unwrap(),
                security::file_identity(&opened).unwrap()
            );
        }
        let file = directory.join("not-a-directory");
        drop(security::create_owner_only_file(&file).unwrap());
        assert!(retain(&file).is_err());
        for invalid in [
            r"relative",
            r"C:relative",
            r"\\server\share",
            r"\\?\UNC\server\share",
            r"\\.\C:\workspace",
        ] {
            assert!(retain(Path::new(invalid)).is_err());
        }
    }
}
