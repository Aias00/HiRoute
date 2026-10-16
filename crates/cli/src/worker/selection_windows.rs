//! Project ordinary drive inputs before the canonical tuple is digested by the
//! shared client. These leases are input preparation, never execution authority.
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf, Prefix};

use hiroute_application_api::{ErrorCode, ErrorV1, WorkerDependenciesSelectRequestV1};
use hiroute_platform::windows::security;

const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

pub(super) struct PreparedSelection {
    pub request: WorkerDependenciesSelectRequestV1,
    // Keep the inspected files and ancestor names stable through the control call.
    _held: Vec<File>,
}

pub(super) fn prepare(
    mut request: WorkerDependenciesSelectRequestV1,
) -> Result<PreparedSelection, ErrorV1> {
    if !request.valid() {
        return Err(ErrorV1::new(ErrorCode::InvalidArguments));
    }
    let mut held = Vec::new();
    for selected in request
        .adapter_path
        .iter_mut()
        .chain(std::iter::once(&mut request.cli_path))
    {
        project(selected, true, &mut held)?;
    }
    if let Some(node) = request.node_path.as_mut() {
        project(node, false, &mut held)?;
    }
    if !request.valid() {
        return Err(ErrorV1::new(ErrorCode::InvalidArguments));
    }
    Ok(PreparedSelection {
        request,
        _held: held,
    })
}

fn project(value: &mut String, resolve_npm: bool, held: &mut Vec<File>) -> Result<(), ErrorV1> {
    let path = Path::new(value);
    let ordinary = drive_path(path).map_err(|_| ErrorV1::new(ErrorCode::InvalidArguments))?;
    if !ordinary {
        // Application checks completed Operations before touching the filesystem.
        // An already canonical tuple must still replay when its files disappeared.
        return Ok(());
    }
    let canonical = retain_canonical(path, held).map_err(dependency_error)?;
    let canonical = if resolve_npm
        && canonical
            .extension()
            .is_some_and(|extension| extension.as_encoded_bytes().eq_ignore_ascii_case(b"cmd"))
    {
        let entry =
            hiroute_integrations::agents::windows_npm_entry_with_inspection(&canonical, |path| {
                // npm declarations commonly begin with "./". Removing only
                // CurDir is lexical; parent traversal and reparses stay rejected.
                let path: PathBuf = path
                    .components()
                    .filter(|component| *component != Component::CurDir)
                    .collect();
                retain_canonical(&path, held).map(|_| ())
            })
            .map_err(|error| {
                if error.kind() == io::ErrorKind::Other {
                    dependency_error(io::ErrorKind::InvalidData.into())
                } else {
                    dependency_error(error)
                }
            })?;
        retain_canonical(&entry, held).map_err(dependency_error)?
    } else {
        canonical
    };
    *value = canonical
        .to_str()
        .ok_or_else(|| ErrorV1::new(ErrorCode::InvalidArguments))?
        .to_owned();
    Ok(())
}

/// Accept local disk spellings only. UNC/device aliases and ADS are not local
/// dependency inputs, even if the Windows path library considers them absolute.
fn drive_path(path: &Path) -> io::Result<bool> {
    let mut components = path.components();
    let ordinary = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(_) => true,
            Prefix::VerbatimDisk(_) => false,
            _ => return Err(io::ErrorKind::InvalidInput.into()),
        },
        _ => return Err(io::ErrorKind::InvalidInput.into()),
    };
    if components.next() != Some(Component::RootDir) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut count = 0;
    for component in components {
        let Component::Normal(name) = component else {
            return Err(io::ErrorKind::InvalidInput.into());
        };
        if name.to_str().is_none_or(|name| {
            name.contains(char::is_control)
                || name.contains([':', '/', '\\', '"', '<', '>', '|', '*', '?'])
        }) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        count += 1;
    }
    if count == 0 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(ordinary)
}

fn retain_canonical(path: &Path, held: &mut Vec<File>) -> io::Result<PathBuf> {
    drive_path(path)?;
    let source = security::open_identity(path, false)?;
    let identity = security::file_identity(&source)?;
    for ancestor in path
        .parent()
        .ok_or(io::ErrorKind::InvalidInput)?
        .ancestors()
    {
        let inspected = security::open_identity(ancestor, true)?;
        let lease = OpenOptions::new()
            .read(true)
            .share_mode(3) // FILE_SHARE_READ | FILE_SHARE_WRITE; deny deletion.
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(ancestor)?;
        if security::file_identity(&inspected)? != security::file_identity(&lease)?
            || !lease.metadata()?.is_dir()
            || lease.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(io::ErrorKind::InvalidData.into());
        }
        held.extend([inspected, lease]);
    }
    let lease = OpenOptions::new()
        .read(true)
        .share_mode(1) // Deny writes and deletion until the request completes.
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if security::file_identity(&lease)? != identity {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let canonical = std::fs::canonicalize(path)?;
    if drive_path(&canonical)? {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let reopened = security::open_identity(&canonical, false)?;
    if security::file_identity(&reopened)? != identity {
        return Err(io::ErrorKind::InvalidData.into());
    }
    held.extend([source, lease, reopened]);
    Ok(canonical)
}

fn dependency_error(error: io::Error) -> ErrorV1 {
    let (code, key) = match error.kind() {
        io::ErrorKind::NotFound => (
            ErrorCode::CapabilityUnavailable,
            "worker.dependencies.missing",
        ),
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => (
            ErrorCode::CapabilityUnavailable,
            "worker.dependencies.invalid",
        ),
        _ => (
            ErrorCode::ObservationUnavailable,
            "worker.dependencies.unavailable",
        ),
    };
    let mut failure = ErrorV1::new(code);
    failure.message_key = key.into();
    failure
}

#[cfg(test)]
#[path = "selection_windows_tests.rs"]
mod tests;
