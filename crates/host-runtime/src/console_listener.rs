//! Persisted management exposure. Only service startup binds the configured listener.
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::{Path, PathBuf},
};
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleListenerMode {
    #[default]
    Off,
    Loopback,
    Network,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsolePortMode {
    #[default]
    Automatic,
    Explicit,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleListenerDesiredV1 {
    pub mode: ConsoleListenerMode,
    pub port_mode: ConsolePortMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<IpAddr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advertise_address: Option<IpAddr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default)]
    pub allow_plaintext_network: bool,
}
impl ConsoleListenerDesiredV1 {
    pub fn loopback() -> Self {
        Self {
            mode: ConsoleListenerMode::Loopback,
            ..Self::default()
        }
    }
    pub fn validate(&self) -> Result<IpAddr, ConsoleListenerError> {
        if matches!(self.port_mode, ConsolePortMode::Explicit) && self.port.is_none_or(|p| p == 0)
            || matches!(self.port_mode, ConsolePortMode::Automatic) && self.port.is_some()
        {
            return Err(ConsoleListenerError);
        }
        match self.mode {
            ConsoleListenerMode::Off | ConsoleListenerMode::Loopback
                if self
                    .address
                    .is_none_or(|a| a == IpAddr::V4(Ipv4Addr::LOCALHOST))
                    && self.advertise_address.is_none() =>
            {
                Ok(IpAddr::V4(Ipv4Addr::LOCALHOST))
            }
            ConsoleListenerMode::Network if self.allow_plaintext_network => {
                let address = self.address.ok_or(ConsoleListenerError)?;
                let advertised = self.advertise_address.unwrap_or(address);
                if address.is_loopback()
                    || address.is_multicast()
                    || advertised.is_loopback()
                    || (self.advertise_address.is_some() && advertised.is_unspecified())
                    || advertised.is_multicast()
                    || advertised == IpAddr::V4(Ipv4Addr::BROADCAST)
                    || advertised.is_ipv4() != address.is_ipv4()
                {
                    return Err(ConsoleListenerError);
                }
                Ok(address)
            }
            _ => Err(ConsoleListenerError),
        }
    }
    pub fn validate_configuration(&self) -> Result<(), ConsoleListenerError> {
        let address = self.validate()?;
        if self.mode == ConsoleListenerMode::Network
            && address.is_unspecified()
            && self.advertise_address.is_none()
        {
            return Err(ConsoleListenerError);
        }
        Ok(())
    }
    /// The actual port comes from the socket, never from pending desired settings.
    pub fn advertised_socket(&self, bound: SocketAddr) -> SocketAddr {
        SocketAddr::new(self.advertise_address.unwrap_or(bound.ip()), bound.port())
    }
    pub fn reserve(&self) -> Result<Option<TcpListener>, ConsoleListenerError> {
        let address = self.validate()?;
        if self.mode == ConsoleListenerMode::Off {
            return Ok(None);
        }
        let listener = match self.port_mode {
            ConsolePortMode::Explicit => TcpListener::bind(SocketAddr::new(
                address,
                self.port.ok_or(ConsoleListenerError)?,
            )),
            ConsolePortMode::Automatic => TcpListener::bind(SocketAddr::new(address, 5838))
                .or_else(|_| TcpListener::bind(SocketAddr::new(address, 0))),
        }
        .map_err(|_| ConsoleListenerError)?;
        Ok(Some(listener))
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleListenerConfigV1 {
    pub schema: String,
    pub desired: ConsoleListenerDesiredV1,
    pub applied_address: Option<SocketAddr>,
    pub failure_code: Option<String>,
}
impl Default for ConsoleListenerConfigV1 {
    fn default() -> Self {
        Self {
            schema: "hiroute.console-listener/v1".into(),
            desired: ConsoleListenerDesiredV1::default(),
            applied_address: None,
            failure_code: None,
        }
    }
}
#[derive(Debug, thiserror::Error)]
#[error("CONSOLE_LISTENER_UNAVAILABLE")]
pub struct ConsoleListenerError;
#[derive(Clone)]
pub struct ConsoleListenerStore {
    root: PathBuf,
}
impl ConsoleListenerStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }
    pub fn path(&self) -> PathBuf {
        self.root.join("console-listener.json")
    }
    pub fn read(&self) -> Result<ConsoleListenerConfigV1, ConsoleListenerError> {
        let mut file = match hiroute_platform::private_fs::open_private_file(&self.path()) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ConsoleListenerConfigV1::default());
            }
            Err(_) => return Err(ConsoleListenerError),
        };
        hiroute_platform::private_fs::verify_single_link(&file)
            .map_err(|_| ConsoleListenerError)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ConsoleListenerError)?;
        if bytes.len() > 16 * 1024 {
            return Err(ConsoleListenerError);
        }
        let config: ConsoleListenerConfigV1 =
            serde_json::from_slice(&bytes).map_err(|_| ConsoleListenerError)?;
        if config.schema != "hiroute.console-listener/v1"
            || config
                .failure_code
                .as_deref()
                .is_some_and(|c| c != "CONSOLE_LISTENER_UNAVAILABLE")
        {
            return Err(ConsoleListenerError);
        }
        config.desired.validate()?;
        Ok(config)
    }
    fn write(&self, config: &ConsoleListenerConfigV1) -> Result<(), ConsoleListenerError> {
        hiroute_platform::private_fs::prepare_storage_directory(&self.root)
            .map_err(|_| ConsoleListenerError)?;
        let bytes = serde_json::to_vec(config).map_err(|_| ConsoleListenerError)?;
        hiroute_platform::durable::write_private_atomic(&self.path(), &bytes)
            .map_err(|_| ConsoleListenerError)
    }
    pub fn configure(
        &self,
        desired: ConsoleListenerDesiredV1,
    ) -> Result<ConsoleListenerConfigV1, ConsoleListenerError> {
        desired.validate_configuration()?;
        let mut config = self.read()?;
        config.desired = desired;
        self.write(&config)?;
        Ok(config)
    }
    pub fn mark_applied(&self, address: Option<SocketAddr>) -> Result<(), ConsoleListenerError> {
        let mut c = self.read()?;
        c.applied_address = address;
        c.failure_code = None;
        self.write(&c)
    }
    pub fn mark_failed(&self) -> Result<(), ConsoleListenerError> {
        let mut c = self.read()?;
        c.applied_address = None;
        c.failure_code = Some("CONSOLE_LISTENER_UNAVAILABLE".into());
        self.write(&c)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn off_by_default_and_explicit_network_confirmation_precedes_writes() {
        let root = tempfile::tempdir().unwrap();
        let store = ConsoleListenerStore::new(root.path());
        assert_eq!(store.read().unwrap().desired.mode, ConsoleListenerMode::Off);
        assert!(store.read().unwrap().desired.reserve().unwrap().is_none());
        let mut desired = ConsoleListenerDesiredV1 {
            mode: ConsoleListenerMode::Network,
            address: Some("0.0.0.0".parse().unwrap()),
            advertise_address: Some("192.0.2.1".parse().unwrap()),
            ..Default::default()
        };
        assert!(store.configure(desired.clone()).is_err());
        assert!(!store.path().exists());
        desired.allow_plaintext_network = true;
        store.configure(desired).unwrap();
        assert_eq!(
            store.read().unwrap().desired.mode,
            ConsoleListenerMode::Network
        );
    }
    #[test]
    fn network_advertisement_is_concrete_and_does_not_change_the_actual_port() {
        let mut desired = ConsoleListenerDesiredV1 {
            mode: ConsoleListenerMode::Network,
            address: Some("0.0.0.0".parse().unwrap()),
            allow_plaintext_network: true,
            ..Default::default()
        };
        assert!(desired.validate_configuration().is_err());
        for invalid in [
            "0.0.0.0",
            "127.0.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "2001:db8::1",
        ] {
            desired.advertise_address = Some(invalid.parse().unwrap());
            assert!(desired.validate().is_err(), "{invalid}");
        }
        desired.advertise_address = Some("192.0.2.1".parse().unwrap());
        assert!(desired.validate().is_ok());
        assert_eq!(
            desired.advertised_socket("0.0.0.0:43210".parse().unwrap()),
            "192.0.2.1:43210".parse().unwrap()
        );
    }
    #[test]
    fn loopback_port_collision_fails_and_auto_falls_back() {
        let held = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = held.local_addr().unwrap().port();
        let d = ConsoleListenerDesiredV1 {
            port_mode: ConsolePortMode::Explicit,
            port: Some(port),
            ..ConsoleListenerDesiredV1::loopback()
        };
        assert!(d.reserve().is_err());
        let automatic = ConsoleListenerDesiredV1::loopback()
            .reserve()
            .unwrap()
            .unwrap();
        assert!(automatic.local_addr().unwrap().ip().is_loopback());
    }
}
