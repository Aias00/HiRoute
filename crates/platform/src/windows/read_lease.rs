//! A borrowed read retains the complete inspected pathname without changing ACLs.

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::Path;

use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

use super::{diagnostic_files, path, security};

pub struct ReadLease {
    file: File,
    parents: Vec<File>,
    parent_ids: Vec<(u32, u64)>,
    file_id: (u32, u64),
    leaf: OsString,
}

/// Open a regular, single-link file through held reparse-free directories.
/// The file denies data writes/deletion and every ancestor denies deletion.
/// This requests read access only and never adopts a borrowed file's ACL.
pub fn open_read_lease(location: &Path) -> io::Result<ReadLease> {
    let (file, parents, leaf) = path::retained_read(location)?;
    let parent_ids = parents
        .iter()
        .map(security::file_identity)
        .collect::<io::Result<Vec<_>>>()?;
    let file_id = security::file_identity(&file)?;
    let lease = ReadLease {
        file,
        parents,
        parent_ids,
        file_id,
        leaf,
    };
    lease.verify()?;
    Ok(lease)
}

impl ReadLease {
    pub fn file(&self) -> &File {
        &self.file
    }

    /// Reopen relative to the final retained directory, never via a fresh path.
    pub fn reopen_file(&self) -> io::Result<File> {
        path::relative(
            self.parents.last().ok_or_else(unsafe_object)?,
            &self.leaf,
            false,
            GENERIC_READ,
            FILE_SHARE_READ,
            false,
            None,
        )
    }

    /// Recheck held objects and the final name. Callers separately compare
    /// before/after metadata, permission snapshots and their bounded content.
    pub fn verify(&self) -> io::Result<()> {
        for (parent, identity) in self.parents.iter().zip(&self.parent_ids) {
            diagnostic_files::stat(parent, true)?;
            if security::file_identity(parent)? != *identity {
                return Err(unsafe_object());
            }
        }
        let reopened = self.reopen_file()?;
        for file in [&self.file, &reopened] {
            diagnostic_files::stat(file, false)?;
            if security::file_identity(file)? != self.file_id
                || security::file_link_count(file)? != 1
            {
                return Err(unsafe_object());
            }
        }
        Ok(())
    }
}

fn unsafe_object() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "read lease object changed")
}
