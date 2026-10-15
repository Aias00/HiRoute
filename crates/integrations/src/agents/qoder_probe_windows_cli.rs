//! Account-free Git Bash command controls with the actual candidate CLI.
//! This does not run Qoder, change its shell selection or borrow a user profile.
#[cfg(test)]
mod tests {
    use super::super::*;
    use hiroute_platform::{file_security, windows::security};
    use std::io::{Read, Seek, SeekFrom};

    // These fixed facts describe the first captured line; they never authorize
    // a shell or retain its arbitrary version text. Unknown remains unknown.
    #[derive(Debug, Eq, PartialEq)]
    struct VersionFacts {
        bytes: usize,
        lines: usize,
        utf8: bool,
        empty: bool,
        first_line_empty: bool,
        gnu_header: bool,
        gnu_header_present: bool,
        msys_suffix: bool,
        triplet: &'static str,
        bom: bool,
        nul: bool,
    }

    fn version_facts(bytes: &[u8]) -> VersionFacts {
        let text = std::str::from_utf8(bytes).ok();
        let first = text
            .and_then(|text| text.lines().next())
            .unwrap_or_default();
        let triplet = first
            .rsplit_once(" (")
            .and_then(|(_, tail)| tail.strip_suffix(')'));
        let triplet = match triplet {
            Some("x86_64-pc-msys") => "msys-x64",
            Some("aarch64-pc-msys") => "msys-arm64",
            Some("i686-pc-msys") => "msys-x86",
            Some("x86_64-pc-cygwin") => "cygwin-x64",
            Some("x86_64-pc-linux-gnu" | "x86_64-unknown-linux-gnu") => "linux-x64",
            Some("x86_64-w64-mingw32") => "mingw-x64",
            Some(_) => "unknown",
            None => "absent",
        };
        VersionFacts {
            bytes: bytes.len(),
            lines: text.map_or(0, |text| text.lines().count()),
            utf8: text.is_some(),
            empty: bytes.is_empty(),
            first_line_empty: first.is_empty(),
            gnu_header: first.starts_with("GNU bash, version "),
            gnu_header_present: text.is_some_and(|text| {
                text.lines()
                    .any(|line| line.starts_with("GNU bash, version "))
            }),
            msys_suffix: first.contains("-pc-msys"),
            triplet,
            bom: bytes.starts_with(&[0xef, 0xbb, 0xbf]),
            nul: bytes.contains(&0),
        }
    }

    fn version_diagnostic_controls() {
        let known = b"GNU bash, version 5.2.37(1)-release (x86_64-pc-msys)\r\nCopyright\r\n";
        let facts = version_facts(known);
        assert!(facts.gnu_header && facts.gnu_header_present && facts.msys_suffix);
        assert_eq!(facts.triplet, "msys-x64");
        assert!(facts.utf8 && !facts.empty && !facts.bom && !facts.nul);
        assert_eq!(facts.lines, 2);
        let empty = version_facts(b"");
        assert!(empty.empty && empty.first_line_empty && !empty.gnu_header);
        assert_eq!(empty.triplet, "absent");
        let linux = version_facts(b"GNU bash, version 5.2.37(1)-release (x86_64-pc-linux-gnu)\n");
        assert!(linux.gnu_header && !linux.msys_suffix);
        assert_eq!(linux.triplet, "linux-x64");
        let prefixed = version_facts(b"\nGNU bash, version 5.2.37(1)-release (x86_64-pc-msys)\n");
        assert!(prefixed.first_line_empty && prefixed.gnu_header_present && !prefixed.gnu_header);
        let unknown = version_facts(b"GNU bash, version 5.2.37(1)-release (unknown)\n");
        assert_eq!(unknown.triplet, "unknown");
        assert!(!unknown.msys_suffix);
        assert!(
            version_facts(b"\xef\xbb\xbfGNU bash, version 5.2.37(1)-release (x86_64-pc-msys)\n")
                .bom
        );
        assert!(!version_facts(b"\xff").utf8);
        assert!(version_facts(b"GNU bash\0").nul);
        assert!(!version_facts(b"arbitrary shell (x86_64-pc-msys)\n").gnu_header);
    }

    fn selected(name: &str) -> PathBuf {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .expect("required explicit native command control image")
    }

    fn image(file: &File) -> (u16, CanonicalDigest) {
        let mut file = file.try_clone().unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        file.take(256 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .unwrap();
        assert!((64..=256 * 1024 * 1024).contains(&bytes.len()));
        assert!(bytes.starts_with(b"MZ"));
        let offset = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
        assert!(offset <= bytes.len() - 6 && offset <= 1024 * 1024);
        assert!(&bytes[offset..offset + 4] == b"PE\0\0");
        let machine = u16::from_le_bytes(bytes[offset + 4..offset + 6].try_into().unwrap());
        assert!(
            matches!(machine, 0x8664 | 0xaa64),
            "actual Windows native PE image"
        );
        (machine, CanonicalDigest::of_bytes(&bytes))
    }

    fn run(bash: &Path, args: &[&str], root: &Path, label: &str) -> (i32, Vec<u8>, bool) {
        use hiroute_platform::windows::job::Stream;
        let output = private_file(&root.join(format!("{label}.stdout"))).unwrap();
        let error = private_file(&root.join(format!("{label}.stderr"))).unwrap();
        let mut command = Command::new(bash);
        command
            .args(args)
            .env_clear()
            .env("HOME", root)
            .env("TMPDIR", root)
            .env(
                "PATH",
                super::super::super::native_ingress_probe::native_probe_path(),
            )
            .current_dir(root);
        let child = super::super::super::windows_runtime::spawn(
            &command,
            false,
            Stream::Null,
            Stream::File(&output),
            Stream::File(&error),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let code = loop {
            assert!(output.metadata().unwrap().len() <= 128 * 1024);
            assert!(error.metadata().unwrap().len() <= 128 * 1024);
            if let Some(code) = child.exit_code().unwrap()
                && child.empty().unwrap()
            {
                break code;
            }
            assert!(
                Instant::now() < deadline,
                "actual Bash control must exit and naturally empty its Job"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(child.empty().unwrap());
        let read = |file: &File| {
            let mut file = file.try_clone().unwrap();
            file.seek(SeekFrom::Start(0)).unwrap();
            let mut bytes = Vec::new();
            file.take(128 * 1024 + 1).read_to_end(&mut bytes).unwrap();
            assert!(bytes.len() <= 128 * 1024);
            bytes
        };
        (code, read(&output), read(&error).is_empty())
    }

    #[test]
    #[ignore = "explicit actual Git for Windows Bash and source-bound candidate hiroute.exe; required CI selects exactly this case"]
    fn windows_qoder_bash_executes_the_same_cli_with_literal_drive_path() {
        version_diagnostic_controls();
        let selected_bash = selected("HIROUTE_VALIDATION_GIT_BASH_BINARY");
        let bash =
            super::super::super::managed_launch::windows_executable_path(&selected_bash).unwrap();
        let cli = super::super::super::managed_launch::windows_executable_path(&selected(
            "HIROUTE_WORKER_CLI_BINARY",
        ))
        .unwrap();
        let bash_file = security::open_identity(&bash, false).unwrap();
        let cli_file = security::open_identity(&cli, false).unwrap();
        let bash_identity = security::file_identity(&bash_file).unwrap();
        let cli_identity = security::file_identity(&cli_file).unwrap();
        let bash_security = file_security::capture(&bash_file).unwrap();
        let cli_security = file_security::capture(&cli_file).unwrap();
        let (bash_machine, bash_digest) = image(&bash_file);
        let (cli_machine, cli_digest) = image(&cli_file);
        assert_eq!(
            cli_machine, 0x8664,
            "the selected released-target candidate is x64"
        );
        let root = super::super::super::probe_files::temporary("hiroute-qoder-bash-").unwrap();
        let (code, version, stderr_empty) = run(&bash, &["--version"], root.path(), "version");
        assert_eq!(code, 0);
        assert!(stderr_empty);
        let facts = version_facts(&version);
        eprintln!(
            "native Qoder Bash version diagnostic: bytes={} lines={} utf8={} empty={} first_line_empty={} gnu_header={} gnu_header_present={} msys_suffix={} triplet={} bom={} nul={}",
            facts.bytes,
            facts.lines,
            facts.utf8,
            facts.empty,
            facts.first_line_empty,
            facts.gnu_header,
            facts.gnu_header_present,
            facts.msys_suffix,
            facts.triplet,
            facts.bom,
            facts.nul,
        );
        let version = std::str::from_utf8(&version).unwrap();
        assert!(
            version.lines().next().is_some_and(
                |line| line.starts_with("GNU bash, version ") && line.contains("-pc-msys")
            ),
            "explicit image must be actual Git for Windows MSYS Bash, not WSL or another shell"
        );
        let expected = cli_baseline(&cli, root.path()).unwrap();
        let special = root.path().join("literal space ' ; $probe & 中文");
        private_dir(&special).unwrap();
        let target = special.join("hiroute ' literal.exe");
        let mut copy = private_file(&target).unwrap();
        let mut source = cli_file.try_clone().unwrap();
        source.seek(SeekFrom::Start(0)).unwrap();
        std::io::copy(&mut source, &mut copy).unwrap();
        copy.sync_all().unwrap();
        drop(copy);
        let held_copy = security::open_owner_only(&target, false).unwrap();
        assert_eq!(image(&held_copy), (cli_machine, cli_digest.clone()));
        let copy_identity = security::file_identity(&held_copy).unwrap();
        let copy_security = file_security::capture(&held_copy).unwrap();
        let old = format!(
            "& '{}' schema list --output json",
            target.to_str().unwrap().replace('\'', "''")
        );
        let (old_exit, old_stdout, _) = run(&bash, &["-n", "-c", &old], root.path(), "old-parse");
        assert_eq!(
            old_exit, 2,
            "the original PowerShell command is not a Bash program"
        );
        assert!(old_stdout.is_empty());
        let canonical = std::fs::canonicalize(&target).unwrap();
        for (index, path) in [&target, &canonical].into_iter().enumerate() {
            let command = cli_command(path).unwrap();
            assert!(!command.starts_with("& ") && !command.starts_with("'\\\\?\\"));
            assert!(
                command.contains("'\\''"),
                "a literal apostrophe needs Bash escaping"
            );
            let (code, bytes, stderr_empty) = run(
                &bash,
                &["-c", &command],
                root.path(),
                &format!("new-{index}"),
            );
            assert_eq!(code, 0);
            assert!(stderr_empty);
            assert!(
                std::str::from_utf8(&bytes).unwrap().trim() == expected.trim(),
                "actual Bash must execute this same CLI and preserve its complete schema envelope"
            );
        }
        let alias = root.path().join("foreign-alias");
        hiroute_platform_test_support::junction::create(&alias, &special).unwrap();
        assert!(cli_command(&alias.join(target.file_name().unwrap())).is_err());
        for (path, held, identity, descriptor, digest) in [
            (
                &bash,
                &bash_file,
                bash_identity,
                &bash_security,
                &bash_digest,
            ),
            (&cli, &cli_file, cli_identity, &cli_security, &cli_digest),
            (
                &target,
                &held_copy,
                copy_identity,
                &copy_security,
                &cli_digest,
            ),
        ] {
            assert_eq!(
                security::file_identity(&security::open_identity(path, false).unwrap()).unwrap(),
                identity
            );
            assert!(file_security::capture(held).unwrap() == *descriptor);
            assert!(image(held).1 == *digest);
        }
        eprintln!(
            "native Qoder Bash command control: bash-pe-machine={bash_machine} cli-pe-machine={cli_machine} bash-msys=true old-parse-exit={old_exit} new-cli-exact=true native-identities-unchanged=true native-permissions-unchanged=true bash-image={bash_digest} cli-image={cli_digest}"
        );
    }
}
