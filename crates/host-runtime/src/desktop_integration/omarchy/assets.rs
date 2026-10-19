//! Release-owned, deterministic desktop files. Exec escaping follows the Desktop Entry spec.
use super::{Asset, Context, Error, ItemKind, files};
use std::path::Path;

pub const PACKAGED_FILES: [&str; 6] = [
    "hiroute.svg",
    "hiroute.status/manifest.json",
    "hiroute.status/Panel.qml",
    "hiroute.status/Summary.js",
    "hiroute.status/i18n/en.json",
    "hiroute.status/i18n/zh.json",
];
impl Context {
    pub fn assets_from(&self, directory: &Path) -> Result<Vec<Asset>, Error> {
        let cli = desktop_argument(&self.stable_cli())?;
        let mut assets = Vec::new();
        for (file, name, zh, command) in [
            (
                "hiroute-codex-routed.desktop",
                "Codex · Difficulty routing",
                "Codex · 难度路由",
                format!(
                    "omarchy-launch-tui --app-id=org.omarchy.agent {cli} integrations omarchy launch --agent codex -- --approve-for-me"
                ),
            ),
            (
                "hiroute-claude-code-routed.desktop",
                "Claude Code · Difficulty routing",
                "Claude Code · 难度路由",
                format!(
                    "omarchy-launch-tui --app-id=org.omarchy.agent {cli} integrations omarchy launch --agent claude-code -- --permission-mode auto"
                ),
            ),
            (
                "hiroute-console.desktop",
                "HiRoute",
                "HiRoute",
                format!("{cli} integrations omarchy open-console"),
            ),
        ] {
            let bytes = format!("[Desktop Entry]\nVersion=1.0\nType=Application\nName={name}\nName[zh]={zh}\nName[zh_CN]={zh}\nExec={command}\nIcon=hiroute\nTerminal=false\nStartupNotify=true\nCategories=Development;Utility;\n").into_bytes();
            assets.push(Asset {
                path: self.home.join(".local/share/applications").join(file),
                kind: ItemKind::DesktopEntry,
                bytes,
            });
        }
        for name in PACKAGED_FILES {
            let (bytes, _) = files::read(&directory.join(name), 0o644, 512 * 1024)?
                .ok_or(Error::InstallationRequired)?;
            let (path, kind) = if name == "hiroute.svg" {
                (
                    self.home
                        .join(".local/share/icons/hicolor/scalable/apps/hiroute.svg"),
                    ItemKind::Icon,
                )
            } else {
                (
                    self.plugin_directory().join(
                        name.strip_prefix("hiroute.status/")
                            .ok_or(Error::InvalidRecord)?,
                    ),
                    ItemKind::Plugin,
                )
            };
            assets.push(Asset { path, kind, bytes });
        }
        self.check_assets(&assets)?;
        Ok(assets)
    }
}
fn desktop_argument(path: &Path) -> Result<String, Error> {
    let value = path
        .to_str()
        .filter(|s| !s.chars().any(char::is_control))
        .ok_or(Error::InvalidRecord)?;
    // First quote argv, then escape backslashes for the desktop file's string layer.
    // https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html
    let mut quoted = String::new();
    for c in value.chars() {
        if matches!(c, '\\' | '"' | '$' | '`') {
            quoted.push('\\');
        }
        if c == '%' {
            quoted.push('%');
        }
        quoted.push(c);
    }
    Ok(format!("\"{}\"", quoted.replace('\\', "\\\\")))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omarchy_desktop_exec_quotes_a_single_path_without_shell_expansion() {
        assert_eq!(
            desktop_argument(Path::new("/home/space name/a$`\\\"%/hiroute")).unwrap(),
            "\"/home/space name/a\\\\$\\\\`\\\\\\\\\\\\\"%%/hiroute\""
        );
        assert!(desktop_argument(Path::new("/home/new\nline/hiroute")).is_err());
    }
}
