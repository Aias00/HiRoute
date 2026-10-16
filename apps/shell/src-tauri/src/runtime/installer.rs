//! NSIS invokes this fixed native adapter; verified package installation still uses the CLI stdin verbs.
use super::service;
use hiroute_host_runtime::standalone_install::{Package, windows};
use hiroute_platform::windows::{security, shell_login, user_path};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::PathBuf,
    time::Duration,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ShellInstall {
    schema: String,
    cli_path_added: bool,
    tray_enabled: bool,
}
fn read_preferences(root: &std::path::Path) -> Result<Option<ShellInstall>, &'static str> {
    let mut file = match security::open_identity(&root.join("shell-install.json"), false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        _ => return Err("INSTALL_OWNERSHIP_CONFLICT"),
    };
    let mut bytes = vec![];
    Read::by_ref(&mut file)
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "INSTALL_OWNERSHIP_CONFLICT")?;
    if bytes.len() > 4096 {
        return Err("INSTALL_OWNERSHIP_CONFLICT");
    }
    let value: ShellInstall =
        serde_json::from_slice(&bytes).map_err(|_| "INSTALL_OWNERSHIP_CONFLICT")?;
    if value.schema != "hiroute.windows-shell-install/v1" {
        return Err("INSTALL_OWNERSHIP_CONFLICT");
    }
    Ok(Some(value))
}
pub(super) fn tray_enabled() -> bool {
    program_root()
        .ok()
        .and_then(|root| read_preferences(&root).ok().flatten())
        .is_none_or(|record| record.tray_enabled)
}
fn options() -> Result<i32, &'static str> {
    let root = hiroute_platform::windows::background::local_app_data()
        .map_err(|_| "INSTALL_DIRECTORY_INVALID")?
        .join("Programs/HiRoute");
    let Some(preferences) = read_preferences(&root)? else {
        return Ok(3);
    };
    let layout = hiroute_host_runtime::StandaloneLayout::from_environment()
        .map_err(|_| "INSTALLATION_UNAVAILABLE")?;
    let definition = hiroute_platform::windows::background::read_definition(
        &layout.data_root.join("service/windows-background.json"),
    )
    .map_err(|_| "INSTALLATION_UNAVAILABLE")?;
    let login = hiroute_platform::windows::task::autostart(&definition)
        .map_err(|_| "SHELL_LOGIN_UNAVAILABLE")?
        && shell_login::enabled(&root.join("hiroute-shell.exe"))
            .map_err(|_| "SHELL_LOGIN_UNAVAILABLE")?;
    Ok(i32::from(login) | (i32::from(preferences.tray_enabled) << 1))
}
pub(super) fn program_root() -> Result<PathBuf, &'static str> {
    let expected = hiroute_platform::windows::background::local_app_data()
        .map_err(|_| "INSTALL_DIRECTORY_INVALID")?
        .join("Programs/HiRoute");
    let exe = std::env::current_exe().map_err(|_| "INSTALL_DIRECTORY_INVALID")?;
    let actual = exe.parent().ok_or("INSTALL_DIRECTORY_INVALID")?;
    let held = security::open_identity(actual, true).map_err(|_| "INSTALL_DIRECTORY_INVALID")?;
    let expected =
        security::open_identity(&expected, true).map_err(|_| "INSTALL_DIRECTORY_INVALID")?;
    if security::file_identity(&held).map_err(|_| "INSTALL_DIRECTORY_INVALID")?
        != security::file_identity(&expected).map_err(|_| "INSTALL_DIRECTORY_INVALID")?
    {
        return Err("INSTALL_DIRECTORY_INVALID");
    }
    Ok(actual.to_owned())
}
fn preflight() -> Result<(), String> {
    windows::preflight().map_err(|error| error.to_string())?;
    let path = hiroute_platform::windows::background::local_app_data()
        .map_err(|_| "INSTALL_DIRECTORY_INVALID")?
        .join("Programs/HiRoute");
    for ancestor in path.ancestors() {
        match security::open_identity(ancestor, true) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("INSTALL_DIRECTORY_INVALID".into()),
        }
    }
    Ok(())
}
fn request(cli: &std::path::Path, verb: &str, value: Value) -> Result<Value, String> {
    let bytes = serde_json::to_vec(&value).map_err(|_| "INSTALL_REQUEST_INVALID")?;
    let (success, value) = service::execute_reply(
        cli,
        &[verb, "--request-stdin"],
        Some(&bytes),
        Duration::from_secs(600),
    )
    .map_err(str::to_owned)?;
    if !success {
        return Err(
            serde_json::to_string(&value["error"]).unwrap_or_else(|_| "INSTALL_FAILED".into())
        );
    }
    Ok(value)
}
fn install(args: &[String]) -> Result<Value, String> {
    windows::preflight().map_err(|error| error.to_string())?;
    let root = program_root()?;
    let package = root.join("standalone");
    Package::read(&package).map_err(|_| "INSTALL_PACKAGE_INVALID")?;
    super::install_transaction::record_generated().map_err(|error| error.to_string())?;
    let enabled = !args.iter().any(|arg| arg == "--no-login-start");
    request(
        &package.join("hiroute.exe"),
        "__windows-install",
        json!({"schema":"hiroute.windows-install-request/v1", "package_directory":package,"autostart":enabled,"console_loopback":true,"allow_legacy_stop":args.iter().any(|arg| arg == "--allow-legacy-stop")}),
    )?;
    let metadata = root.join("shell-install.json");
    let previous = read_preferences(&root)?.is_some_and(|value| value.cli_path_added);
    let added = user_path::configure(true).map_err(|_| "USER_PATH_UNAVAILABLE")?;
    let bytes = serde_json::to_vec(&ShellInstall {
        schema: "hiroute.windows-shell-install/v1".into(),
        cli_path_added: previous || added,
        tray_enabled: !args.iter().any(|a| a == "--no-tray"),
    })
    .map_err(|_| "INSTALL_FAILED")?;
    if hiroute_platform::durable::write_private_atomic(&metadata, &bytes).is_err() {
        if added {
            let _ = user_path::configure(false);
        }
        return Err("INSTALL_FAILED".into());
    }
    super::commands::login_set(enabled)?;
    service::manager(&["service", "start", "--output", "json"])?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "SERVICE_NOT_READY")?
        .block_on(service::wait_ready())?;
    super::install_transaction::commit().map_err(|error| error.to_string())?;
    Ok(json!({"installed":true,"started":true}))
}
fn uninstall(args: &[String]) -> Result<Value, String> {
    windows::preflight().map_err(|error| error.to_string())?;
    let root = program_root()?;
    super::install_transaction::validate_installed().map_err(|error| error.to_string())?;
    let previous = read_preferences(&root)?;
    let layout = hiroute_host_runtime::StandaloneLayout::from_environment()
        .map_err(|_| "INSTALLATION_UNAVAILABLE")?;
    let cli = if layout
        .marker_path
        .try_exists()
        .map_err(|_| "INSTALLATION_UNAVAILABLE")?
    {
        service::cli()?
    } else {
        let package = root.join("standalone");
        Package::read(&package).map_err(|_| "INSTALL_PACKAGE_INVALID")?;
        package.join("hiroute.exe")
    };
    let result = request(
        &cli,
        "__windows-uninstall",
        json!({"schema":"hiroute.windows-uninstall-request/v1", "remove_data":args.iter().any(|arg| arg == "--remove-data"),"allow_legacy_stop":args.iter().any(|arg| arg == "--allow-legacy-stop")}),
    )?;
    shell_login::remove().map_err(|_| "SHELL_LOGIN_UNAVAILABLE")?;
    if previous.is_some_and(|v| v.cli_path_added) {
        user_path::configure(false).map_err(|_| "USER_PATH_UNAVAILABLE")?;
    }
    Ok(result)
}
pub fn run_if_requested() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verb = args.first()?;
    if !verb.starts_with("--installer-") {
        return None;
    }
    if verb == "--installer-mutex" {
        if args.len() != 1 || hiroute_platform::windows::install::require_x64().is_err() {
            return Some(1);
        }
        let Ok(name) = hiroute_platform::windows::install::desktop_mutex_name() else {
            return Some(1);
        };
        let mut output = std::io::stdout().lock();
        return Some(
            if write!(output, "{name}")
                .and_then(|()| output.flush())
                .is_ok()
            {
                0
            } else {
                1
            },
        );
    }
    if verb == "--installer-options" {
        return Some(
            if args.len() == 1 && hiroute_platform::windows::install::require_x64().is_ok() {
                options().unwrap_or(8)
            } else {
                8
            },
        );
    }
    if verb == "--installer-prepare" {
        let result = if args.len() == 2 {
            super::install_transaction::prepare(std::path::Path::new(&args[1]))
        } else {
            Err(std::io::ErrorKind::InvalidInput.into())
        };
        return Some(if result.is_ok() { 0 } else { 1 });
    }
    if matches!(
        verb.as_str(),
        "--installer-rollback" | "--installer-recover"
    ) {
        let result =
            windows::recover_pending().and_then(|()| super::install_transaction::rollback());
        let _ = super::updater::resume_admission();
        return Some(if result.is_ok() { 0 } else { 1 });
    }
    let result = if args.iter().skip(1).any(|a| {
        !matches!(
            a.as_str(),
            "--no-login-start" | "--no-tray" | "--allow-legacy-stop" | "--remove-data"
        )
    }) {
        Err("INSTALL_ARGUMENTS_INVALID".into())
    } else if hiroute_platform::windows::install::require_x64().is_err() {
        Err("INSTALL_ARCHITECTURE_UNSUPPORTED".into())
    } else {
        match verb.as_str() {
            "--installer-preflight" => preflight().map(|_| json!({"ready":true})),
            "--installer-install" => install(&args),
            "--installer-uninstall" => uninstall(&args),
            _ => Err("INSTALL_ARGUMENTS_INVALID".into()),
        }
    };
    let (exit, text) = match result {
        Ok(value) => (0, value.to_string()),
        Err(error) => (
            if error.contains("DRAIN_UNSUPPORTED") {
                20
            } else if error.contains("AGENT_RESTORE_REQUIRED") {
                21
            } else if error.contains("UPGRADE_ACTIVE_WORK") {
                22
            } else {
                1
            },
            error,
        ),
    };
    let _ = writeln!(std::io::stdout().lock(), "{text}");
    Some(exit)
}
