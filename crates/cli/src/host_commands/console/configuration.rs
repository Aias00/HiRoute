use hiroute_application_api::ErrorCode;
use hiroute_host_runtime::{ConsoleListenerDesiredV1, ConsoleListenerMode, ConsolePortMode};
use std::net::IpAddr;

pub(super) fn open_options(options: &[String]) -> Result<(bool, bool), ErrorCode> {
    let (mut browser, mut local) = (false, false);
    for flag in options {
        match flag.as_str() {
            "--browser" if !browser => browser = true,
            "--loopback-only" if !local => local = true,
            _ => return Err(ErrorCode::InvalidArguments),
        }
    }
    Ok((browser, local))
}

pub(super) fn parse(
    options: &[String],
    initialization: bool,
) -> Result<ConsoleListenerDesiredV1, ErrorCode> {
    let mut desired = ConsoleListenerDesiredV1::loopback();
    let mut mode = None;
    let mut address = None;
    let mut it = options.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--mode" if initialization && mode.is_none() => {
                mode = Some(match it.next().map(String::as_str) {
                    Some("off") => ConsoleListenerMode::Off,
                    Some("loopback") => ConsoleListenerMode::Loopback,
                    Some("network") => ConsoleListenerMode::Network,
                    _ => return Err(ErrorCode::InvalidArguments),
                });
            }
            "--listen" if address.is_none() => address = Some(ip(it.next())?),
            "--advertise-address" if desired.advertise_address.is_none() => {
                desired.advertise_address = Some(ip(it.next())?);
            }
            "--port" if desired.port.is_none() => {
                desired.port_mode = ConsolePortMode::Explicit;
                desired.port = Some(
                    it.next()
                        .ok_or(ErrorCode::InvalidArguments)?
                        .parse()
                        .map_err(|_| ErrorCode::InvalidArguments)?,
                );
            }
            "--allow-plaintext-network" if !desired.allow_plaintext_network => {
                desired.allow_plaintext_network = true
            }
            _ => return Err(ErrorCode::InvalidArguments),
        }
    }
    desired.mode = mode.unwrap_or_else(|| {
        if address.is_some_and(|a: IpAddr| !a.is_loopback()) {
            ConsoleListenerMode::Network
        } else {
            ConsoleListenerMode::Loopback
        }
    });
    desired.address = address;
    if desired.mode == ConsoleListenerMode::Network && !desired.allow_plaintext_network {
        return Err(ErrorCode::ActionRequired);
    }
    desired
        .validate_configuration()
        .map_err(|_| ErrorCode::InvalidArguments)?;
    Ok(desired)
}
fn ip(value: Option<&String>) -> Result<IpAddr, ErrorCode> {
    value
        .ok_or(ErrorCode::InvalidArguments)?
        .parse()
        .map_err(|_| ErrorCode::InvalidArguments)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(value: &str) -> Vec<String> {
        value.split_whitespace().map(str::to_owned).collect()
    }
    #[test]
    fn launch_selection_is_explicit_and_never_treated_as_browser_open() {
        assert_eq!(open_options(&[]).unwrap(), (false, false));
        assert_eq!(
            open_options(&args("--loopback-only")).unwrap(),
            (false, true)
        );
        assert_eq!(
            open_options(&args("--browser --loopback-only")).unwrap(),
            (true, true)
        );
        assert_eq!(
            open_options(&args("--loopback-only --browser")).unwrap(),
            (true, true)
        );
        for invalid in [
            "--loopback-only --loopback-only",
            "--browser --browser",
            "--remote",
            "--listen 127.0.0.1",
        ] {
            assert_eq!(
                open_options(&args(invalid)).unwrap_err(),
                ErrorCode::InvalidArguments
            );
        }
    }
    #[test]
    fn initialization_validates_scope_port_and_plaintext_before_writing() {
        assert_eq!(
            parse(&[], true).unwrap().mode,
            ConsoleListenerMode::Loopback
        );
        assert_eq!(
            parse(&args("--mode off"), true).unwrap().mode,
            ConsoleListenerMode::Off
        );
        assert_eq!(
            parse(&args("--mode network --listen 192.0.2.1"), true).unwrap_err(),
            ErrorCode::ActionRequired
        );
        for bad in [
            "--port 0",
            "--port 65536",
            "--port 7000 --port 8000",
            "--mode loopback --listen 192.0.2.1",
            "--listen ::1",
            "--listen 0.0.0.0 --allow-plaintext-network",
            "--listen 0.0.0.0 --advertise-address ::1 --allow-plaintext-network",
        ] {
            assert!(parse(&args(bad), true).is_err(), "{bad}");
        }
        let configured = parse(&args("--mode network --listen 0.0.0.0 --port 7000 --advertise-address 192.0.2.1 --allow-plaintext-network"), true).unwrap();
        assert_eq!(configured.port, Some(7000));
        assert_eq!(configured.mode, ConsoleListenerMode::Network);
        assert!(parse(&args("--mode loopback"), false).is_err());
    }
}
