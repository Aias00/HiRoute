//! Borrowed native credentials are readable external objects, not managed files.
//! Retain the inspected parent and regular file while decoding and publishing an
//! access lease. Read permission is sufficient; the native descriptor is never
//! rewritten. Identity and permission changes invalidate that admission.

use hiroute_platform::file_security::{self, WindowsPermissions};
use hiroute_platform::windows::security;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::os::windows::fs::FileExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use zeroize::Zeroizing;

#[derive(Eq, PartialEq)]
struct Facts {
    identity: (u32, u64),
    length: u64,
    modified: SystemTime,
    permissions: WindowsPermissions,
}

impl Facts {
    fn read(file: &File) -> io::Result<Self> {
        if security::file_link_count(file)? != 1 {
            return Err(unsafe_source());
        }
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(unsafe_source());
        }
        Ok(Self {
            identity: security::file_identity(file)?,
            length: metadata.len(),
            modified: metadata.modified()?,
            permissions: file_security::capture(file)?,
        })
    }
}

pub(crate) struct ReadGuard {
    path: PathBuf,
    parent_path: PathBuf,
    name: OsString,
    parent: File,
    parent_identity: (u32, u64),
    file: File,
    facts: Facts,
    decoded_digest: Option<[u8; 32]>,
}

impl ReadGuard {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        Self::open_with(path, || {})
    }

    #[cfg(test)]
    pub(crate) fn open_after_parent(path: &Path, after_parent: impl FnOnce()) -> io::Result<Self> {
        Self::open_with(path, after_parent)
    }

    fn open_with(path: &Path, after_parent: impl FnOnce()) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(unsafe_source());
        }
        let parent_path = path.parent().ok_or_else(unsafe_source)?.to_owned();
        let name = path.file_name().ok_or_else(unsafe_source)?.to_owned();
        // Both traversal and the leaf use the platform's no-reparse kernel
        // resolver. The leaf is opened relative to this exact inspected parent.
        let parent = security::open_identity(&parent_path, true)?;
        let parent_identity = security::file_identity(&parent)?;
        after_parent();
        let file = security::open_identity_at(&parent, &name, false)?;
        let facts = Facts::read(&file)?;
        let guard = Self {
            path: path.to_owned(),
            parent_path,
            name,
            parent,
            parent_identity,
            file,
            facts,
            decoded_digest: None,
        };
        guard.verify()?;
        Ok(guard)
    }

    pub(crate) fn canonical_path(&self) -> io::Result<PathBuf> {
        self.verify()?;
        // Preserve existing path-digest bytes. Canonicalization alone grants no
        // authority: its returned object must match our held native file.
        let canonical = self.path.canonicalize()?;
        let file = security::open_identity(&canonical, false)?;
        if security::file_identity(&file)? != self.facts.identity {
            return Err(unsafe_source());
        }
        self.verify()?;
        Ok(canonical)
    }

    pub(crate) fn read(&mut self, limit: u64) -> io::Result<Zeroizing<Vec<u8>>> {
        self.verify()?;
        if self.facts.length > limit {
            return Err(unsafe_source());
        }
        let mut bytes = Zeroizing::new(Vec::new());
        self.file.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit || bytes.len() as u64 != self.facts.length {
            return Err(unsafe_source());
        }
        self.decoded_digest = Some(Sha256::digest(bytes.as_slice()).into());
        self.verify()?;
        Ok(bytes)
    }

    pub(crate) fn metadata(&self) -> io::Result<std::fs::Metadata> {
        self.file.metadata()
    }

    pub(crate) fn verify(&self) -> io::Result<()> {
        self.verify_object_facts()?;
        if let Some(expected) = self.decoded_digest {
            // Sharing writes is required for native-client refresh. The current
            // bytes must still match those decoded, even if an in-place writer
            // preserves file length and restores the modification timestamp.
            // Positional reads use only the retained object and are bounded by
            // the decoded regular-file length; no pathname supplies the bytes.
            let mut hash = Sha256::new();
            let mut buffer = Zeroizing::new([0_u8; 8192]);
            let mut offset = 0;
            while offset < self.facts.length {
                let count = usize::try_from((self.facts.length - offset).min(buffer.len() as u64))
                    .map_err(|_| unsafe_source())?;
                let count = self.file.seek_read(&mut buffer[..count], offset)?;
                if count == 0 {
                    return Err(unsafe_source());
                }
                hash.update(&buffer[..count]);
                offset += count as u64;
            }
            let mut extra = Zeroizing::new([0_u8; 1]);
            if self.file.seek_read(&mut extra[..], offset)? != 0
                || <[u8; 32]>::from(hash.finalize()) != expected
            {
                return Err(unsafe_source());
            }
            self.verify_object_facts()?;
        }
        Ok(())
    }

    fn verify_object_facts(&self) -> io::Result<()> {
        // An ancestor may be renamed while held. Reopen the original locator
        // without following reparses and reject a replacement parent, even when
        // its file has identical content, length and modification timestamp.
        let parent = security::open_identity(&self.parent_path, true)?;
        if security::file_identity(&parent)? != self.parent_identity
            || security::file_identity(&self.parent)? != self.parent_identity
        {
            return Err(unsafe_source());
        }
        let current = security::open_identity_at(&self.parent, &self.name, false)?;
        if security::file_identity(&current)? != self.facts.identity
            || Facts::read(&self.file)? != self.facts
        {
            return Err(unsafe_source());
        }
        Ok(())
    }
}

fn unsafe_source() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "native source changed or unsafe",
    )
}
