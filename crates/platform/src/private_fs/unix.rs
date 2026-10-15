use super::{DirectoryError, FileMode};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub(super) fn prepare_storage_directory(path: &Path) -> Result<(), DirectoryError> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DirectoryError::NotDirectory);
        }
    } else {
        fs::create_dir_all(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DirectoryError::NotDirectory);
        }
    }
    Ok(())
}

pub fn create_directory(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)
}

pub fn create_file(path: &Path, mode: FileMode) -> io::Result<File> {
    OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .mode(match mode {
            FileMode::Data => 0o600,
            FileMode::Executable => 0o700,
        })
        .open(path)
}

pub fn open_regular_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(file)
}

pub(super) fn open_private_file(path: &Path) -> io::Result<File> {
    open_regular_file(path)
}

pub(super) fn verify_single_link(file: &File) -> io::Result<()> {
    if file.metadata()?.nlink() != 1 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}

pub(super) fn write_material(path: &Path, bytes: &[u8], mode: FileMode) -> io::Result<()> {
    create_file(path, mode)?.write_all(bytes)
}

/// A freshly created root with its original identity held until owner-led removal.
/// No Drop cleanup: the caller decides whether live processes still need this root.
pub struct OwnedDirectory {
    path: PathBuf,
    held: File,
}

impl OwnedDirectory {
    pub fn create(path: &Path) -> io::Result<Self> {
        create_directory(path)?;
        Ok(Self {
            path: path.to_owned(),
            held: File::open(path)?,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn remove(&mut self) -> io::Result<()> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let held = self.held.metadata()?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || (metadata.dev(), metadata.ino()) != (held.dev(), held.ino())
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        fs::remove_dir_all(&self.path)
    }
}
