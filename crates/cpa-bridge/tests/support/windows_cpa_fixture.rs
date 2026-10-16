//! Native, credential-free process contract fixture; build directly with rustc in Windows CI.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::{env, fs, io::Read, thread, time::Duration};

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--descendant") {
        let root = PathBuf::from(&args[1]);
        let _child = Command::new(env::current_exe().unwrap())
            .args(["--grandchild".as_ref(), root.as_os_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        fs::write(root.join("descendant.pid"), std::process::id().to_string()).unwrap();
        hold();
    }
    if args.first().is_some_and(|arg| arg == "--grandchild") {
        let root = PathBuf::from(&args[1]);
        fs::write(root.join("grandchild.pid"), std::process::id().to_string()).unwrap();
        hold();
    }
    assert_eq!(args.len(), 4);
    assert_eq!(args[0], "--config");
    assert_eq!(args[2], "--local-model");
    assert_eq!(args[3], "--local-password-stdin");
    let configuration = PathBuf::from(&args[1]);
    let root = configuration.parent().unwrap();
    let mut secret = Vec::new();
    std::io::stdin().read_to_end(&mut secret).unwrap();
    assert_eq!(secret, b"synthetic_cpa_stdin_only_secret_0123456789");
    secret.fill(0);
    assert!(env::var_os("SystemRoot").is_some());
    assert_eq!(env::var_os("SystemRoot"), env::var_os("WINDIR"));
    let temporary = PathBuf::from(env::var_os("TEMP").unwrap());
    assert_eq!(Some(temporary.as_os_str()), env::var_os("TMP").as_deref());
    assert_eq!(temporary, root.join("tmp"));
    assert!(temporary.is_dir());
    assert_eq!(env::var("NO_PROXY").unwrap(), "localhost,127.0.0.1");
    assert!(env::var_os("OPENAI_API_KEY").is_none());
    assert!(env::var_os("MANAGEMENT_PASSWORD").is_none());
    assert!(env::var_os("BASH_ENV").is_none());
    fs::write(
        root.join("bootstrap.ok"),
        b"stdin-eof-and-explicit-environment",
    )
    .unwrap();
    if fs::read(&configuration).unwrap() == b"tree" {
        let _child = Command::new(env::current_exe().unwrap())
            .args(["--descendant".as_ref(), root.as_os_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        for _ in 0..500 {
            if root.join("descendant.pid").is_file() && root.join("grandchild.pid").is_file() {
                // Intentionally leave descendants alive after the root exits.
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("native descendants did not start");
    }
}

fn hold() -> ! {
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
