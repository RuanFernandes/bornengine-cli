#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

use std::os::unix::process::CommandExt;

struct CliFixture {
    project: TempDir,
    fake_perry: TempDir,
    log: PathBuf,
}

impl CliFixture {
    fn new(with_assets: bool) -> Self {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join("package.json"),
            r#"{"name":"fixture","dependencies":{"@bornengine/engine":"0.7.0"}}"#,
        )
        .unwrap();
        fs::write(project.path().join("main.ts"), "export {};\n").unwrap();
        if with_assets {
            fs::create_dir_all(project.path().join("assets")).unwrap();
            fs::write(project.path().join("assets/player.png"), [1, 2, 3]).unwrap();
        }

        let fake_perry = tempfile::tempdir().unwrap();
        let executable = fake_perry.path().join("perry");
        fs::write(
            &executable,
            r##"#!/bin/sh
if [ "$1" = "compile" ] && [ "$2" = "--help" ]; then
    printf 'Target platform: linux (default: native)\n'
    exit 0
fi
if [ "$1" = "compile" ] && [ "$PERRY_STREAM_TEST" = "1" ]; then
    printf 'PERRY_STREAM_STARTED\n'
    printf 'PERRY_STREAM_DIAGNOSTIC\n' >&2
    sleep 0.5
    exit 19
fi
if [ "$1" = "compile" ] && [ "$PERRY_FILL_PIPES" = "1" ]; then
    head -c 262144 /dev/zero | tr '\\000' 'o'
    head -c 262144 /dev/zero | tr '\\000' 'e' >&2
    exit 19
fi
{
    printf 'cwd=%s\n' "$PWD"
    for argument in "$@"; do printf 'arg=%s\n' "$argument"; done
} > "$PERRY_TEST_LOG"
output=''
previous=''
for argument in "$@"; do
    if [ "$previous" = "--output" ]; then output=$argument; fi
    previous=$argument
done
if [ -n "$output" ]; then printf 'generated binary' > "$output"; fi
printf 'generated object' > "$PWD/main_ts.o"
if [ "$1" = "compile" ]; then exit "${PERRY_COMPILE_EXIT:-19}"; fi
exit 19
"##,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();

        Self {
            log: project.path().join("perry.log"),
            project,
            fake_perry,
        }
    }

    fn command(&self, arguments: &[&str]) -> Output {
        self.command_with_env(arguments, None)
    }

    fn command_with_env(&self, arguments: &[&str], override_env: Option<(&str, &str)>) -> Output {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::iter::once(self.fake_perry.path().to_path_buf())
            .chain(std::env::split_paths(&path))
            .collect::<Vec<_>>();
        let mut command = Command::new(env!("CARGO_BIN_EXE_bornengine"));
        command
            .current_dir(self.project.path())
            .env("PATH", std::env::join_paths(path).unwrap())
            .env("PERRY_TEST_LOG", &self.log)
            .args(arguments);
        if let Some((name, value)) = override_env {
            command.env(name, value);
        }
        command.output().unwrap()
    }

    fn spawn_with_env(&self, arguments: &[&str], environment: &[(&str, &str)]) -> Child {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::iter::once(self.fake_perry.path().to_path_buf())
            .chain(std::env::split_paths(&path))
            .collect::<Vec<_>>();
        let mut command = Command::new(env!("CARGO_BIN_EXE_bornengine"));
        command
            .current_dir(self.project.path())
            .env("PATH", std::env::join_paths(path).unwrap())
            .env("PERRY_TEST_LOG", &self.log)
            .envs(environment.iter().copied())
            .args(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.process_group(0);
        command.spawn().unwrap()
    }

    fn clean_and_recorded_files(&self) -> Vec<String> {
        let manifest = self.project.path().join(".bornengine/builds/manifest.json");
        let value: Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
        value["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect()
    }

    fn clean(&self) {
        let output = self.command(&["clean"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn assert_failed_compile_artifacts_are_recorded(mode: &str) {
    let fixture = CliFixture::new(false);
    let output = fixture.command(&[mode, "main.ts"]);

    assert!(!output.status.success());
    let recorded = fixture.clean_and_recorded_files();
    assert!(
        recorded.iter().any(|path| path.ends_with("main_ts.o")),
        "{recorded:?}"
    );
    assert!(
        recorded.iter().any(|path| path.ends_with("fixture")),
        "{recorded:?}"
    );

    fixture.clean();
    for path in recorded {
        assert!(!fixture.project.path().join(path).exists());
    }
}

#[test]
fn failed_build_outputs_are_recorded_for_clean() {
    assert_failed_compile_artifacts_are_recorded("build");
}

#[test]
fn failed_run_compile_outputs_are_recorded_for_clean() {
    assert_failed_compile_artifacts_are_recorded("run");
}

#[test]
fn run_outputs_remain_recorded_when_launching_the_game_fails() {
    let fixture = CliFixture::new(false);
    let output = fixture.command_with_env(&["run", "main.ts"], Some(("PERRY_COMPILE_EXIT", "0")));

    assert!(!output.status.success());
    let recorded = fixture.clean_and_recorded_files();
    assert!(
        recorded.iter().any(|path| path.ends_with("main_ts.o")),
        "{recorded:?}"
    );
    assert!(
        recorded.iter().any(|path| path.ends_with("fixture")),
        "{recorded:?}"
    );

    fixture.clean();
    for path in recorded {
        assert!(!fixture.project.path().join(path).exists());
    }
}

#[test]
fn native_build_streams_compiler_output_before_perry_exits_and_keeps_failure_context() {
    let fixture = CliFixture::new(false);
    let mut child = fixture.spawn_with_env(&["build", "main.ts"], &[("PERRY_STREAM_TEST", "1")]);
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let stderr = child.stderr.take().unwrap();
    let (first_line_tx, first_line_rx) = mpsc::channel();
    let stdout_reader = thread::spawn(move || {
        let mut stdout = stdout;
        let mut line = String::new();
        if stdout.read_line(&mut line).is_ok() {
            let _ = first_line_tx.send(line);
        }
        let mut remaining = Vec::new();
        let _ = stdout.read_to_end(&mut remaining);
    });
    let stderr_reader = thread::spawn(move || {
        let mut stderr = stderr;
        let mut contents = Vec::new();
        let _ = stderr.read_to_end(&mut contents);
        contents
    });

    let first_line = first_line_rx.recv_timeout(Duration::from_millis(250));
    let was_running_after_stream = child.try_wait().unwrap().is_none();
    let status = wait_for_child(&mut child, Duration::from_secs(3)).unwrap();
    stdout_reader.join().unwrap();
    let stderr = String::from_utf8_lossy(&stderr_reader.join().unwrap()).into_owned();

    assert!(
        first_line.is_ok(),
        "compiler output should reach stdout before the compile process exits"
    );
    assert!(first_line.unwrap().contains("PERRY_STREAM_STARTED"));
    assert!(
        was_running_after_stream,
        "Perry should still be compiling when its first line is visible"
    );
    assert!(!status.success());
    assert!(stderr.contains("PERRY_STREAM_DIAGNOSTIC"), "{stderr}");
    assert_eq!(stderr.matches("PERRY_STREAM_DIAGNOSTIC").count(), 1);
}

#[test]
fn native_build_drains_large_stdout_and_stderr_without_deadlock() {
    let fixture = CliFixture::new(false);
    let mut child = fixture.spawn_with_env(&["build", "main.ts"], &[("PERRY_FILL_PIPES", "1")]);
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let stdout_reader = thread::spawn(move || {
        let mut stdout = stdout;
        let mut contents = Vec::new();
        let _ = stdout.read_to_end(&mut contents);
        contents
    });
    let stderr_reader = thread::spawn(move || {
        let mut stderr = stderr;
        let mut contents = Vec::new();
        let _ = stderr.read_to_end(&mut contents);
        contents
    });

    let status = wait_for_child(&mut child, Duration::from_secs(5)).unwrap();
    let stdout = stdout_reader.join().unwrap();
    let stderr = stderr_reader.join().unwrap();
    assert!(!status.success());
    assert!(
        stdout.len() >= 262_144,
        "only {} stdout bytes streamed",
        stdout.len()
    );
    assert!(
        stderr.len() >= 262_144,
        "only {} stderr bytes streamed",
        stderr.len()
    );
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> std::io::Result<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let process_group = format!("-{}", child.id());
            let _ = Command::new("kill")
                .args(["-KILL", "--", process_group.as_str()])
                .status();
            return child.wait();
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn dev_watches_assets_from_project_root_without_packing_stale_copies() {
    let fixture = CliFixture::new(true);
    let output = fixture.command(&["dev", "main.ts", "--watch"]);
    let project_root = fs::canonicalize(fixture.project.path()).unwrap();

    assert_eq!(output.status.code(), Some(19));
    let log = fs::read_to_string(&fixture.log).unwrap();
    assert!(
        log.contains(&format!("cwd={}\n", project_root.display())),
        "{log}"
    );
    assert!(
        log.contains(&format!("arg={}\n", project_root.join("assets").display())),
        "{log}"
    );
    assert!(log.contains("arg=--watch\n"), "{log}");

    let dev_root = project_root.join(".perry-dev");
    let manifests = find_named_file(&dev_root, "assets.manifest.json");
    assert!(manifests.is_empty(), "stale asset manifests: {manifests:?}");

    let recorded = fixture.clean_and_recorded_files();
    assert!(
        recorded.iter().any(|path| path.ends_with("main_ts.o")),
        "{recorded:?}"
    );
    fixture.clean();
    assert!(
        !find_named_file(&dev_root, "main_ts.o")
            .iter()
            .any(|path| path.exists())
    );
}

fn find_named_file(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(find_named_file(&path, name));
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            found.push(path);
        }
    }
    found
}
