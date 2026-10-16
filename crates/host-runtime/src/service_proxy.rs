//! Private handoff from the standalone CLI to its independently launched service.
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const SCHEMA: &str = "hiroute.standalone-proxy-environment/v1";
const MAX_BYTES: u64 = 16 * 1024;
const KEYS: [&str; 6] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
];

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceProxyEnvironment {
    schema: String,
    variables: BTreeMap<String, String>,
}

impl std::fmt::Debug for ServiceProxyEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServiceProxyEnvironment([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Error)]
#[error("STANDALONE_PROXY_ENVIRONMENT_INVALID")]
pub struct ServiceProxyError;

impl ServiceProxyEnvironment {
    pub fn capture(
        values: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Result<Self, ServiceProxyError> {
        let mut variables = BTreeMap::new();
        for (key, value) in values {
            #[cfg(windows)]
            let key = key.to_str().map(str::to_ascii_uppercase);
            #[cfg(not(windows))]
            let key = key.to_str().map(str::to_owned);
            if let Some(key) = key.filter(|key| KEYS.contains(&key.as_str())) {
                let value = value.into_string().map_err(|_| ServiceProxyError)?;
                #[cfg(windows)]
                if variables.get(&key).is_some_and(|old| old != &value) {
                    return Err(ServiceProxyError);
                }
                variables.insert(key.to_owned(), value);
            }
        }
        let result = Self {
            schema: SCHEMA.into(),
            variables,
        };
        result.validate()?;
        Ok(result)
    }

    pub fn variables(&self) -> impl Iterator<Item = (OsString, OsString)> + '_ {
        self.variables.iter().map(|(key, value)| {
            #[cfg(windows)]
            let key = key.to_ascii_uppercase();
            (key.into(), value.into())
        })
    }

    fn validate(&self) -> Result<(), ServiceProxyError> {
        if self.schema != SCHEMA
            || self.variables.iter().any(|(key, value)| {
                !KEYS.contains(&key.as_str()) || value.contains(['\0', '\r', '\n'])
            })
            || serde_json::to_vec(self)
                .map_err(|_| ServiceProxyError)?
                .len() as u64
                > MAX_BYTES
        {
            return Err(ServiceProxyError);
        }
        #[cfg(windows)]
        {
            let mut logical = BTreeMap::new();
            for (key, value) in &self.variables {
                if logical.insert(key.to_ascii_uppercase(), value).is_some() {
                    return Err(ServiceProxyError);
                }
            }
        }
        Ok(())
    }

    /// This lives beside the fixed installation marker, independent of shell XDG overrides.
    pub fn path(home: &Path) -> PathBuf {
        home.join(".local/share/hiroute/service/proxy-environment.json")
    }

    pub fn store(&self, home: &Path) -> Result<(), ServiceProxyError> {
        self.validate()?;
        let path = Self::path(home);
        validate_directories(home, true)?;
        if let Some(file) = open_private(&path)? {
            drop(file);
        }
        hiroute_platform::durable::write_private_atomic(
            &path,
            &serde_json::to_vec(self).map_err(|_| ServiceProxyError)?,
        )
        .map_err(|_| ServiceProxyError)
    }

    pub fn load(home: &Path) -> Result<Option<Self>, ServiceProxyError> {
        if !validate_directories(home, false)? {
            return Ok(None);
        }
        let Some(file) = open_private(&Self::path(home))? else {
            return Ok(None);
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ServiceProxyError)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(ServiceProxyError);
        }
        let result: Self = serde_json::from_slice(&bytes).map_err(|_| ServiceProxyError)?;
        result.validate()?;
        Ok(Some(result))
    }
}

fn validate_directories(home: &Path, create: bool) -> Result<bool, ServiceProxyError> {
    if !home.is_absolute() {
        return Err(ServiceProxyError);
    }
    let mut path = home.to_path_buf();
    for component in ["", ".local", "share", "hiroute", "service"] {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    return Err(ServiceProxyError);
                }
                #[cfg(windows)]
                hiroute_platform::windows::security::open_identity(&path, true)
                    .map_err(|_| ServiceProxyError)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                hiroute_platform::private_fs::create_directory(&path)
                    .map_err(|_| ServiceProxyError)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(ServiceProxyError),
        }
    }
    Ok(true)
}

fn open_private(path: &Path) -> Result<Option<File>, ServiceProxyError> {
    let opened = hiroute_platform::private_fs::open_regular_file(path);
    let file = match opened {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ServiceProxyError),
    };
    let metadata = file.metadata().map_err(|_| ServiceProxyError)?;
    if !metadata.is_file() {
        return Err(ServiceProxyError);
    }
    #[cfg(windows)]
    if hiroute_platform::windows::security::file_link_count(&file).map_err(|_| ServiceProxyError)?
        != 1
    {
        return Err(ServiceProxyError);
    }
    Ok(Some(file))
}

#[cfg(test)]
#[path = "service_proxy_tests.rs"]
mod tests;
