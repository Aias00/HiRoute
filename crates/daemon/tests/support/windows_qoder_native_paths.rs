//! Test-owned Qoder Bash resources. A spelling is useful only after native identity.
#[cfg(windows)]
use hiroute_platform::{file_security, windows::security};
#[cfg(windows)]
use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf, Prefix},
};

pub(crate) fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(windows)]
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "native Qoder fixture path")
}

#[cfg(windows)]
fn parts(path: &Path) -> io::Result<(u8, Vec<&OsStr>)> {
    let mut parts = path.components();
    let drive = match parts.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) if drive.is_ascii_alphabetic() => {
                drive
            }
            _ => return Err(invalid()),
        },
        _ => return Err(invalid()),
    };
    if parts.next() != Some(Component::RootDir) {
        return Err(invalid());
    }
    let mut names = Vec::new();
    for part in parts {
        let Component::Normal(name) = part else {
            return Err(invalid());
        };
        if name
            .as_encoded_bytes()
            .iter()
            .any(|byte| matches!(byte, 0 | b':' | b'\r' | b'\n'))
        {
            return Err(invalid());
        }
        names.push(name);
    }
    if names.is_empty() {
        return Err(invalid());
    }
    Ok((drive, names))
}

#[cfg(windows)]
pub(crate) fn same_directory_spelling(left: &Path, right: &Path) -> io::Result<bool> {
    let (drive, names) = parts(left)?;
    let (other_drive, other_names) = parts(right)?;
    Ok(drive.eq_ignore_ascii_case(&other_drive)
        && names.len() == other_names.len()
        && names.iter().zip(other_names).all(|(name, other)| {
            name.as_encoded_bytes()
                .eq_ignore_ascii_case(other.as_encoded_bytes())
        }))
}

#[cfg(windows)]
pub(crate) struct BashFile {
    file: File,
    lease: Option<security::ReadLease>,
    parents: Vec<(PathBuf, File, (u32, u64))>,
    ordinary: PathBuf,
    identity: (u32, u64),
    permissions: file_security::WindowsPermissions,
    links: u32,
    quoted: String,
}

#[cfg(windows)]
impl BashFile {
    pub(crate) fn retain(path: &Path) -> io::Result<Self> {
        parts(path)?;
        let lease = security::open_read_lease(path)?;
        let file = lease.file().try_clone()?;
        let mut binding = Self::from_held(path, file, true)?;
        binding.lease = Some(lease);
        binding.verify()?;
        Ok(binding)
    }

    /// Published stable CLI entries are real hardlinks. Retain read-only bytes
    /// and each actual no-reparse ancestor without imposing private-data nlink=1.
    #[allow(
        dead_code,
        reason = "Used by the separate ordinary-model integration harness"
    )]
    pub(crate) fn trusted_cli(path: &Path) -> io::Result<Self> {
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        parts(path)?;
        let mut parents = Vec::new();
        for parent in path
            .ancestors()
            .skip(1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let inspected = security::open_identity(parent, true)?;
            let pinned = OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                .open(parent)?;
            hiroute_platform::windows::diagnostic_files::stat(&pinned, true)?;
            let identity = security::file_identity(&inspected)?;
            if security::file_identity(&pinned)? != identity {
                return Err(invalid());
            }
            parents.push((parent.to_owned(), pinned, identity));
        }
        let inspected = security::open_identity(path, false)?;
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        if security::file_identity(&file)? != security::file_identity(&inspected)? {
            return Err(invalid());
        }
        let mut binding = Self::from_held(path, file, false)?;
        binding.parents = parents;
        binding.verify()?;
        Ok(binding)
    }

    /// Only a new private output beside a retained input is writable. The
    /// input's ancestor leases fix the parent; this handle denies output deletion.
    #[allow(
        dead_code,
        reason = "Used by the separate native-tools library harness"
    )]
    pub(crate) fn created_output(path: &Path, input: &Self) -> io::Result<Self> {
        input.verify()?;
        let parent = path.parent().ok_or_else(invalid)?;
        let input_parent = input.ordinary.parent().ok_or_else(invalid)?;
        let parent_file = security::open_identity(parent, true)?;
        let input_parent_file = security::open_identity(input_parent, true)?;
        if security::file_identity(&parent_file)? != security::file_identity(&input_parent_file)? {
            return Err(invalid());
        }
        let file = security::open_read_write(path)?;
        security::verify_owner_only_handle(&file, false)?;
        Self::from_held(path, file, true)
    }

    fn from_held(path: &Path, file: File, single_link: bool) -> io::Result<Self> {
        parts(path)?;
        let links = security::file_link_count(&file)?;
        if links == 0
            || single_link && links != 1
            || !file.metadata()?.is_file()
            || file.metadata()?.file_attributes() & 0x400 != 0
        {
            return Err(invalid());
        }
        let identity = security::file_identity(&file)?;
        let permissions = file_security::capture(&file)?;
        // Only the no-reparse held object may authorize canonicalization.
        let canonical = std::fs::canonicalize(path)?;
        let (drive, names) = parts(&canonical)?;
        let mut ordinary = PathBuf::from(format!("{}:\\", char::from(drive)));
        for name in names {
            ordinary.push(name);
        }
        if std::fs::canonicalize(&ordinary)? != canonical {
            return Err(invalid());
        }
        let text = ordinary.to_str().ok_or_else(invalid)?.replace('\\', "/");
        if !same_directory_spelling(&ordinary, Path::new(&text))? {
            return Err(invalid());
        }
        for spelling in [&ordinary, Path::new(&text)] {
            let reopened = security::open_identity(spelling, false)?;
            if security::file_identity(&reopened)? != identity
                || security::file_link_count(&reopened)? != links
                || file_security::capture(&reopened)? != permissions
            {
                return Err(invalid());
            }
        }
        Ok(Self {
            file,
            lease: None,
            parents: Vec::new(),
            ordinary,
            identity,
            permissions,
            links,
            quoted: literal(&text),
        })
    }

    pub(crate) fn quoted(&self) -> &str {
        &self.quoted
    }

    pub(crate) fn verify(&self) -> io::Result<()> {
        if let Some(lease) = &self.lease {
            lease.verify()?;
        }
        for (path, parent, identity) in &self.parents {
            hiroute_platform::windows::diagnostic_files::stat(parent, true)?;
            let current = security::open_identity(path, true)?;
            if security::file_identity(parent)? != *identity
                || security::file_identity(&current)? != *identity
            {
                return Err(invalid());
            }
        }
        let current = security::open_identity(&self.ordinary, false)?;
        for file in [&self.file, &current] {
            if security::file_identity(file)? != self.identity
                || security::file_link_count(file)? != self.links
                || file_security::capture(file)? != self.permissions
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
