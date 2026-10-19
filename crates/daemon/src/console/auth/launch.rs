//! An owner-issued code is scoped to one established HTTP origin and transport.
use std::net::SocketAddr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchTarget {
    pub origin: String,
    pub loopback_only: bool,
}
impl LaunchTarget {
    pub fn new(address: SocketAddr, loopback_only: bool) -> Self {
        Self {
            origin: format!("http://{}", authority(address)),
            loopback_only,
        }
    }
}

pub(crate) fn authority(address: SocketAddr) -> String {
    if address.port() == 80 {
        match address.ip() {
            std::net::IpAddr::V4(ip) => ip.to_string(),
            std::net::IpAddr::V6(ip) => format!("[{ip}]"),
        }
    } else {
        address.to_string()
    }
}

pub(super) struct Launch {
    pub expires: u64,
    pub target: LaunchTarget,
    pub landing: Option<String>,
}
