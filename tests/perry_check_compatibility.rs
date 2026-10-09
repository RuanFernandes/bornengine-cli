#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use tempfile::TempDir;

struct PerryCheckFixture {
    project: TempDir,
    perry_bin: TempDir,
    args_log: PathBuf,
}

impl PerryCheckFixture {
    fn new() -> Self {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join("package.json"),
            r#"{"name":"fixture","dependencies":{"@bornengine/engine":"0.14.0"}}"#,
        )
        .unwrap();
        fs::write(project.path().join("main.ts"), "export {};\n").unwrap();

        let perry_bin = tempfile::tempdir().unwrap();
        let executable = perry_bin.path().join("perry");
        fs::write(
            &executable,
            r##"#!/bin/sh
if [ "$1" = "compile" ] && [ "$2" = "--help" ]; then
    printf 'Target platform: linux (default: native)\n'
    exit 0
fi
printf '%s\n' "$@" > "$PERRY_ARGS_LOG"
printf 'PERRY_CHECK_STDOUT\n'
printf 'PERRY_CHECK_STDERR\n' >&2
exit "${PERRY_EXIT:-0}"
"##,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();

        Self {
            args_log: project.path().join("perry-args.log"),
            project,
            perry_bin,
        }
    }

    fn command(&self, arguments: &[&str], perry_exit: &str) -> Output {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::iter::once(self.perry_bin.path().to_path_buf())
            .chain(std::env::split_paths(&path))
            .collect::<Vec<_>>();
        Command::new(env!("CARGO_BIN_EXE_bornengine"))
            .current_dir(self.project.path())
            .env("PATH", std::env::join_paths(path).unwrap())
            .env("BORNENGINE_PERRY", self.perry_bin.path().join("perry"))
            .env("PERRY_ARGS_LOG", &self.args_log)
            .env("PERRY_EXIT", perry_exit)
            .args(arguments)
            .output()
            .unwrap()
    }
}

fn assert_check_arguments(log: &str, expected_flags: &[&str]) {
    let arguments = log.lines().collect::<Vec<_>>();
    assert_eq!(arguments.first(), Some(&"check"));
    assert_eq!(
        PathBuf::from(arguments.get(1).unwrap())
            .file_name()
            .unwrap(),
        "main.ts"
    );
    assert_eq!(&arguments[2..], expected_flags);
}

#[test]
fn dependency_scan_streams_perry_report_and_warnings_on_success() {
    let fixture = PerryCheckFixture::new();
    let output = fixture.command(
        &["check", "main.ts", "--check-deps", "--deep-deps", "--all"],
        "0",
    );

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("PERRY_CHECK_STDOUT"));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Perry check completed successfully.")
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("PERRY_CHECK_STDERR"));
    assert_check_arguments(
        &fs::read_to_string(&fixture.args_log).unwrap(),
        &["--check-deps", "--deep-deps", "--all"],
    );
}

#[test]
fn strict_dependency_scan_preserves_perry_warnings_and_exit_code() {
    let fixture = PerryCheckFixture::new();
    let output = fixture.command(&["check", "main.ts", "--check-deps", "--strict"], "7");

    assert_eq!(output.status.code(), Some(7));
    assert!(String::from_utf8_lossy(&output.stdout).contains("PERRY_CHECK_STDOUT"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("PERRY_CHECK_STDERR"));
    assert_check_arguments(
        &fs::read_to_string(&fixture.args_log).unwrap(),
        &["--check-deps", "--strict"],
    );
}
