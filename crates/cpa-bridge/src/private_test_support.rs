//! Fresh test-owned private roots, without adopting the cleanup directory.
//!
//! An elevated Windows runner may give `tempfile`'s directory Administrators
//! ownership and inherited ACEs. Keep it only for cleanup; CPA test objects live
//! in a newly created child with the production native owner/DACL contract.

#[cfg(not(windows))]
pub(crate) use tempfile::{TempDir, tempdir};

#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct TempDir {
    cleanup: tempfile::TempDir,
    private_path: std::path::PathBuf,
}

#[cfg(windows)]
impl TempDir {
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.private_path
    }

    pub(crate) fn keep(self) -> std::path::PathBuf {
        let _cleanup_path = self.cleanup.keep();
        self.private_path
    }
}

#[cfg(windows)]
pub(crate) fn tempdir() -> std::io::Result<TempDir> {
    use hiroute_platform::windows::security;

    let cleanup = tempfile::tempdir()?;
    let private_path = cleanup.path().join("private");
    let directory = security::create_owner_only_directory(&private_path)?;
    security::verify_owner_only_handle(&directory, true)?;
    drop(directory);
    Ok(TempDir {
        cleanup,
        private_path,
    })
}
