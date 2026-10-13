//! Windows equivalent of the existing held-directory contract. ACL admission applies
//! only to objects created here; readable native objects retain their existing ACLs.

use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fs2::FileExt;
use hiroute_platform::windows::diagnostic_files::{self as native, OpenMode};

use super::{FileSafetyError, FileSafetyError as E};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileIdentity(native::Identity);

#[derive(Debug)]
pub struct PrivateDir {
    path: PathBuf,
    file: File,
    identity: FileIdentity,
    scan: Mutex<()>,
}

impl PrivateDir {
    pub fn open_existing(path: &Path) -> Result<Self, FileSafetyError> {
        Self::open(path, false)
    }
    pub fn open_or_create(path: &Path) -> Result<Self, FileSafetyError> {
        Self::open(path, true)
    }

    fn open(path: &Path, create: bool) -> Result<Self, FileSafetyError> {
        let path = std::path::absolute(path).map_err(map_file_error)?;
        let file = native::open_directory(&path, create).map_err(map_directory_error)?;
        let identity = FileIdentity(
            native::stat(&file, true)
                .map_err(map_directory_error)?
                .identity,
        );
        Ok(Self {
            path,
            file,
            identity,
            scan: Mutex::new(()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn identity(&self) -> FileIdentity {
        self.identity
    }

    pub fn recheck_identity(&self) -> Result<(), FileSafetyError> {
        let actual = native::stat(&self.file, true)
            .map_err(map_directory_error)?
            .identity;
        if FileIdentity(actual) != self.identity {
            return Err(E::IdentityChanged);
        }
        Ok(())
    }

    pub fn child_dir(&self, name: &str) -> Result<Self, FileSafetyError> {
        self.recheck_identity()?;
        let file = native::child_directory(&self.file, name).map_err(map_directory_error)?;
        let identity = FileIdentity(
            native::stat(&file, true)
                .map_err(map_directory_error)?
                .identity,
        );
        Ok(Self {
            path: self.path.join(name),
            file,
            identity,
            scan: Mutex::new(()),
        })
    }

    pub fn open_read(&self, name: &str) -> Result<Option<VerifiedFile>, FileSafetyError> {
        match self.open_file(name, OpenMode::Read) {
            Ok(file) => Ok(Some(file)),
            Err(E::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn open_append_or_create(&self, name: &str) -> Result<VerifiedFile, FileSafetyError> {
        self.open_file(name, OpenMode::Write)
    }

    pub fn create_new(&self, name: &str) -> Result<VerifiedFile, FileSafetyError> {
        self.open_file(name, OpenMode::New)
    }

    pub fn open_lock(&self, name: &str) -> Result<VerifiedFile, FileSafetyError> {
        self.open_file(name, OpenMode::Write)
    }

    fn open_file(&self, name: &str, mode: OpenMode) -> Result<VerifiedFile, FileSafetyError> {
        self.recheck_identity()?;
        let file = native::open_file(&self.file, name, mode).map_err(map_file_error)?;
        let stat = native::stat(&file, false).map_err(map_file_error)?;
        Ok(VerifiedFile {
            file,
            name: name.into(),
            identity: FileIdentity(stat.identity),
            len: stat.length,
        })
    }

    pub fn rename_verified(
        &self,
        from: &str,
        to: &str,
        expect: FileIdentity,
    ) -> Result<(), FileSafetyError> {
        self.recheck_identity()?;
        let source = self.open_file(from, OpenMode::Delete)?;
        if source.identity != expect {
            return Err(E::IdentityChanged);
        }
        // Preserve native target ACLs: eligibility is type/single-link, as on Unix.
        // Keep the inspected target object held during the atomic namespace operation.
        let _target = self.open_read(to)?;
        native::rename(&source.file, &self.file, to).map_err(map_file_error)
    }

    pub fn remove_verified(&self, name: &str, expect: FileIdentity) -> Result<(), FileSafetyError> {
        self.recheck_identity()?;
        let file = self.open_file(name, OpenMode::Delete)?;
        if file.identity != expect {
            return Err(E::IdentityChanged);
        }
        native::remove(&file.file).map_err(map_file_error)
    }

    pub fn list_names(&self) -> Result<Vec<String>, FileSafetyError> {
        self.recheck_identity()?;
        let _scan = self.scan.lock().map_err(|_| E::InvalidData)?;
        native::list_names(&self.file).map_err(map_directory_error)
    }

    pub fn sync(&self) -> Result<(), FileSafetyError> {
        // Windows has no fsync(directory) counterpart: FlushFileBuffers on a directory
        // returns AccessDenied. NtCreateFile supplies FILE_WRITE_THROUGH for all held
        // directories/files; create/rename/delete use those same exact handles. This
        // validates that metadata barrier, while payload sync below calls sync_all.
        // https://learn.microsoft.com/en-us/windows/win32/fileio/file-caching
        self.recheck_identity()
    }
}

#[derive(Debug)]
pub struct VerifiedFile {
    file: File,
    name: String,
    identity: FileIdentity,
    len: u64,
}

impl VerifiedFile {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn len(&self) -> u64 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn identity(&self) -> FileIdentity {
        self.identity
    }

    pub fn recheck(&mut self) -> Result<(), FileSafetyError> {
        let actual = native::stat(&self.file, false)
            .map_err(map_file_error)?
            .identity;
        if FileIdentity(actual) != self.identity {
            return Err(E::IdentityChanged);
        }
        Ok(())
    }

    pub fn read_prefix(&mut self, max_bytes: u64) -> Result<Vec<u8>, FileSafetyError> {
        self.read_range(0, self.len.min(max_bytes))
    }

    pub fn read_tail(&mut self, max_bytes: u64) -> Result<Vec<u8>, FileSafetyError> {
        let offset = self.len.saturating_sub(max_bytes);
        self.read_range(offset, self.len - offset)
    }

    fn read_range(&mut self, offset: u64, length: u64) -> Result<Vec<u8>, FileSafetyError> {
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(map_file_error)?;
        let mut bytes = Vec::new();
        (&mut self.file)
            .take(length)
            .read_to_end(&mut bytes)
            .map_err(map_file_error)?;
        Ok(bytes)
    }

    pub fn append(&mut self, bytes: &[u8]) -> Result<(), FileSafetyError> {
        self.recheck()?;
        self.file.seek(SeekFrom::End(0)).map_err(map_file_error)?;
        self.file.write_all(bytes).map_err(map_file_error)?;
        self.len += bytes.len() as u64;
        Ok(())
    }

    pub fn replace_contents(&mut self, bytes: &[u8]) -> Result<(), FileSafetyError> {
        self.recheck()?;
        self.file.set_len(0).map_err(map_file_error)?;
        self.file.seek(SeekFrom::Start(0)).map_err(map_file_error)?;
        self.file.write_all(bytes).map_err(map_file_error)?;
        self.len = bytes.len() as u64;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<(), FileSafetyError> {
        self.file.sync_all().map_err(map_file_error)
    }

    pub fn try_lock_exclusive(&self) -> Result<(), FileSafetyError> {
        FileExt::try_lock_exclusive(&self.file).map_err(|error| {
            if error.kind() == ErrorKind::WouldBlock || error.raw_os_error() == Some(33) {
                E::Locked
            } else {
                map_file_error(error)
            }
        })
    }

    pub fn unlock(&self) -> Result<(), FileSafetyError> {
        FileExt::unlock(&self.file).map_err(map_file_error)
    }
}

fn map_file_error(error: std::io::Error) -> FileSafetyError {
    match error.kind() {
        ErrorKind::NotFound => E::NotFound,
        ErrorKind::AlreadyExists => E::AlreadyExists,
        ErrorKind::InvalidInput | ErrorKind::PermissionDenied => E::UnsafeFile,
        ErrorKind::InvalidData => E::InvalidData,
        _ => E::io(error),
    }
}

fn map_directory_error(error: std::io::Error) -> FileSafetyError {
    match map_file_error(error) {
        E::UnsafeFile => E::UnsafeDirectory,
        error => error,
    }
}
