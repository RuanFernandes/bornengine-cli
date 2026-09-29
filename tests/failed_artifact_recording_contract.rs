#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

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
fn dev_watches_assets_from_project_root_without_packing_stale_copies() {
    let fixture = CliFixture::new(true);
    let output = fixture.command(&["dev", "main.ts", "--watch"]);

    assert_eq!(output.status.code(), Some(19));
    let log = fs::read_to_string(&fixture.log).unwrap();
    assert!(
        log.contains(&format!("cwd={}\n", fixture.project.path().display())),
        "{log}"
    );
    assert!(
        log.contains(&format!(
            "arg={}\n",
            fixture.project.path().join("assets").display()
        )),
        "{log}"
    );
    assert!(log.contains("arg=--watch\n"), "{log}");

    let dev_root = fixture.project.path().join(".perry-dev");
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
