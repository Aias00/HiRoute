//! One complete authority key, published without replacing an existing generation.
use std::{fs::File, io::Read, path::Path};
use zeroize::Zeroizing;

const INVALID: &str = "observation authority key is invalid or unavailable";

pub(super) fn observation_workspace_key(
    storage_root: &Path,
) -> Result<Zeroizing<[u8; 32]>, String> {
    #[cfg(windows)]
    if !storage_root.is_absolute() {
        return Err(INVALID.into());
    }
    let path = storage_root.join("observation-hmac-key");
    for _ in 0..2 {
        match open_key(&path) {
            Ok(mut file) => {
                let metadata = file.metadata().map_err(|_| INVALID)?;
                if !metadata.is_file() || metadata.len() != 32 {
                    return Err(INVALID.into());
                }
                #[cfg(windows)]
                if hiroute_platform::windows::security::file_link_count(&file)
                    .map_err(|_| INVALID)?
                    != 1
                {
                    return Err(INVALID.into());
                }
                let mut key = Zeroizing::new([0u8; 32]);
                file.read_exact(&mut *key).map_err(|_| INVALID)?;
                let mut trailing = Zeroizing::new([0u8; 1]);
                if file.read(&mut *trailing).map_err(|_| INVALID)? != 0 {
                    return Err(INVALID.into());
                }
                return Ok(key);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                #[cfg(windows)]
                let _root =
                    hiroute_platform::windows::security::ensure_private_directory(storage_root)
                        .map_err(|_| INVALID)?;
                #[cfg(not(windows))]
                std::fs::create_dir_all(storage_root).map_err(|_| INVALID)?;
                let mut key = Zeroizing::new([0u8; 32]);
                getrandom::fill(&mut *key).map_err(|_| INVALID)?;
                match hiroute_platform::durable::write_private_new(&path, &*key) {
                    Ok(()) => return Ok(key),
                    // Another legitimate startup published first. Read its exact key.
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(_) => return Err(INVALID.into()),
                }
            }
            Err(_) => return Err(INVALID.into()),
        }
    }
    Err(INVALID.into())
}

#[cfg(windows)]
fn open_key(path: &Path) -> std::io::Result<File> {
    hiroute_platform::windows::security::open_owner_only(path, false)
}

#[cfg(not(windows))]
fn open_key(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authority_key_is_complete_stable_and_never_replaced_by_bad_lengths() {
        let fixture = tempfile::tempdir().unwrap();
        let first = observation_workspace_key(fixture.path()).unwrap();
        assert_eq!(first, observation_workspace_key(fixture.path()).unwrap());
        let path = fixture.path().join("observation-hmac-key");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 32);
        #[cfg(windows)]
        hiroute_platform::windows::security::verify_owner_only(&path, false).unwrap();
        for malformed in [vec![1; 31], vec![2; 33]] {
            std::fs::write(&path, &malformed).unwrap();
            assert!(observation_workspace_key(fixture.path()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), malformed);
        }
    }
}
