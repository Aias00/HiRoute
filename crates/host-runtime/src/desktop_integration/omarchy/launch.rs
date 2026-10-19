//! The only owner of Omarchy's desktop working-directory convention.
use std::path::{Path, PathBuf};
pub fn working_directory(home: &Path, caller: &Path) -> PathBuf {
    let work = home.join("Work");
    if caller == home && work.is_dir() {
        work
    } else {
        caller.to_owned()
    }
}
pub fn valid_loopback_launch_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    value.len() <= 512
        && url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|h| matches!(h, "127.0.0.1" | "[::1]" | "::1"))
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/launch"
        && url.fragment().is_none()
        && {
            let pairs = url.query_pairs().collect::<Vec<_>>();
            pairs.len() == 1
                && pairs[0].0 == "code"
                && pairs[0].1.len() == 64
                && pairs[0].1.bytes().all(|b| b.is_ascii_hexdigit())
        }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omarchy_work_directory_only_changes_home_with_existing_work() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        assert_eq!(working_directory(home, home), home);
        std::fs::create_dir(home.join("Work")).unwrap();
        assert_eq!(working_directory(home, home), home.join("Work"));
        assert_eq!(
            working_directory(home, &home.join("project")),
            home.join("project")
        );
    }
    #[test]
    fn omarchy_console_url_never_accepts_remote_credentials_or_extra_destinations() {
        let code = "a".repeat(64);
        assert!(valid_loopback_launch_url(&format!(
            "http://127.0.0.1:8000/launch?code={code}"
        )));
        for bad in [
            format!("http://example.invalid/launch?code={code}"),
            format!("http://user@127.0.0.1/launch?code={code}"),
            format!("http://127.0.0.1/launch?code={code}&next=x"),
            format!("http://127.0.0.1/launch?code={code}#x"),
        ] {
            assert!(!valid_loopback_launch_url(&bad));
        }
    }
}
