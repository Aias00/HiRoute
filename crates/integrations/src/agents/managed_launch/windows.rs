//! Windows executable observations and managed descriptors use ordinary drive
//! paths. Converting Rust's verbatim spelling requires the same actual file;
//! UNC, device and reparse aliases never become admissible by string rewriting.
use super::{ManagedClaudeLaunchDescriptorV2, ManagedClaudeProcessError};
use hiroute_platform::windows::security;
use std::{
    fs::File,
    io,
    path::{Component, Path, PathBuf, Prefix},
};

struct NativeExecutable {
    path: PathBuf,
    file: File,
}

pub(in crate::agents) fn executable_path(path: &Path) -> io::Result<PathBuf> {
    native_executable(path).map(|value| value.path)
}

fn drive_components(path: &Path) -> io::Result<(u8, impl Iterator<Item = Component<'_>>)> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(invalid());
    };
    let drive = match prefix.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
        _ => return Err(invalid()),
    };
    if !drive.is_ascii_alphabetic() || components.next() != Some(Component::RootDir) {
        return Err(invalid());
    }
    Ok((drive, components))
}

fn native_executable(path: &Path) -> io::Result<NativeExecutable> {
    // Inspect the supplied path before canonicalization can hide a junction or
    // symlink. This is borrowed executable admission, not private-file ownership.
    let _ = drive_components(path)?;
    let file = security::open_identity(path, false)?;
    let canonical = std::fs::canonicalize(path)?;
    let (drive, components) = drive_components(&canonical)?;
    let mut ordinary = PathBuf::from(format!("{}:\\", char::from(drive)));
    let mut has_leaf = false;
    for component in components {
        let Component::Normal(name) = component else {
            return Err(invalid());
        };
        ordinary.push(name);
        has_leaf = true;
    }
    if !has_leaf || std::fs::canonicalize(&ordinary)? != canonical {
        return Err(invalid());
    }
    let roundtrip = security::open_identity(&ordinary, false)?;
    if security::file_identity(&file)? != security::file_identity(&roundtrip)? {
        return Err(invalid());
    }
    Ok(NativeExecutable {
        path: ordinary,
        file,
    })
}

pub(super) fn prepare_paths(
    descriptor: &ManagedClaudeLaunchDescriptorV2,
    trusted: &Path,
) -> Result<(ManagedClaudeLaunchDescriptorV2, Vec<File>), ManagedClaudeProcessError> {
    let prepare = || -> io::Result<_> {
        let trusted = native_executable(trusted)?;
        let helper = native_executable(Path::new(&descriptor.helper_executable))?;
        let executable = native_executable(Path::new(&descriptor.executable))?;
        if security::file_identity(&helper.file)? != security::file_identity(&trusted.file)?
            || executable.path.as_os_str() != std::ffi::OsStr::new(&descriptor.executable)
        {
            return Err(invalid());
        }
        let mut descriptor = descriptor.clone();
        descriptor.helper_executable = helper.path.to_str().ok_or_else(invalid)?.to_owned();
        descriptor
            .validate_trusted_helper(trusted.path.to_str().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
        Ok((descriptor, vec![trusted.file, helper.file, executable.file]))
    };
    prepare().map_err(|_| ManagedClaudeProcessError::Unavailable)
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "native drive executable identity",
    )
}
