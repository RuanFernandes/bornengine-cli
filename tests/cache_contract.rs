use std::fs;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
fn engine_project(root: &std::path::Path) -> std::path::PathBuf {
    let manifest_platform = current_native_platform();
    fs::create_dir_all(
        root.join("node_modules/@bornengine/engine/native")
            .join(manifest_platform),
    )
    .unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"cache-fixture","dependencies":{"@bornengine/engine":"0.8.0"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("perry.toml"),
        "[bornengine]\nnative_profile = \"2d\"\nnative_features = [\"sqlite\", \"scripting\"]\n",
    )
    .unwrap();
    let engine = root.join("node_modules/@bornengine/engine");
    fs::write(
        engine
            .join("native")
            .join(manifest_platform)
            .join("Cargo.toml"),
        "[package]\nname = \"bloom-linux\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    engine
}

#[cfg(unix)]
fn current_native_platform() -> &'static str {
    match std::env::consts::OS {
        "linux" => "linux",
        "windows" => "windows",
        "macos" => "macos",
        other => panic!("unsupported host for the cache warm contract: {other}"),
    }
}

#[test]
fn cache_path_reports_an_override_outside_a_game_project() {
    let directory = tempfile::tempdir().unwrap();
    let override_path = directory.path().join("my-shared-cargo-cache");
    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args(["cache", "path"])
        .current_dir(directory.path())
        .env("CARGO_TARGET_DIR", &override_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(override_path.to_str().unwrap()),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn cache_path_uses_the_standard_user_cache_when_no_override_is_set() {
    let directory = tempfile::tempdir().unwrap();
    let expected = directories::BaseDirs::new()
        .unwrap()
        .cache_dir()
        .join("BornEngine")
        .join("cargo-target");
    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args(["cache", "path"])
        .current_dir(directory.path())
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        expected.to_str().unwrap()
    );
}

#[test]
fn cache_warm_reports_a_missing_installed_engine() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{"name":"cache-fixture","dependencies":{"@bornengine/engine":"0.8.0"}}"#,
    )
    .unwrap();
    fs::write(
        project.path().join("perry.toml"),
        "[bornengine]\nnative_profile = \"2d\"\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args(["cache", "warm"])
        .current_dir(project.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(
        diagnostic.contains("install project dependencies first"),
        "{diagnostic}"
    );
}

#[cfg(unix)]
#[test]
fn cache_warm_uses_the_selected_engine_features_profile_cache_and_jobs_without_perry() {
    let project = tempfile::tempdir().unwrap();
    let engine = engine_project(project.path());
    let bin = project.path().join("fake-bin");
    fs::create_dir_all(&bin).unwrap();
    let cargo = bin.join("cargo");
    let arguments = project.path().join("cargo-args.txt");
    fs::write(
        &cargo,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$BORNENGINE_TEST_CARGO_ARGS\"\nprintf 'TARGET=%s\\nJOBS=%s\\nDEV_OPT=%s\\nINCREMENTAL=%s\\n' \"$CARGO_TARGET_DIR\" \"$CARGO_BUILD_JOBS\" \"$CARGO_PROFILE_DEV_OPT_LEVEL\" \"$CARGO_INCREMENTAL\" >> \"$BORNENGINE_TEST_CARGO_ARGS\"\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&cargo).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&cargo, permissions).unwrap();
    let cache = project.path().join("chosen-cache");

    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args(["cache", "warm", "--release", "--jobs", "4"])
        .current_dir(project.path())
        .env("PATH", &bin)
        .env("CARGO_TARGET_DIR", &cache)
        .env("BORNENGINE_TEST_CARGO_ARGS", &arguments)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = fs::read_to_string(&arguments).unwrap();
    assert!(output.lines().any(|line| line == "--release"), "{output}");
    assert!(
        output.lines().any(|line| line == "--no-default-features"),
        "{output}"
    );
    assert!(output.lines().any(|line| line == "--features"), "{output}");
    assert!(
        output.lines().any(|line| line == "mp3,sqlite,scripting"),
        "{output}"
    );
    assert!(
        output.contains(
            engine
                .join("native")
                .join(current_native_platform())
                .join("Cargo.toml")
                .to_str()
                .unwrap()
        ),
        "{output}"
    );
    assert!(
        output.contains(&format!("TARGET={}\n", cache.display())),
        "{output}"
    );
    assert!(output.contains("JOBS=4"), "{output}");
    assert!(output.contains("DEV_OPT=\nINCREMENTAL=\n"), "{output}");
    assert!(!project.path().join("perry-was-run").exists());

    let fast_output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args(["cache", "warm", "--jobs", "2"])
        .current_dir(project.path())
        .env("PATH", &bin)
        .env("CARGO_TARGET_DIR", &cache)
        .env("BORNENGINE_TEST_CARGO_ARGS", &arguments)
        .env_remove("CARGO_PROFILE_DEV_OPT_LEVEL")
        .env_remove("CARGO_INCREMENTAL")
        .output()
        .unwrap();
    assert!(
        fast_output.status.success(),
        "{}",
        String::from_utf8_lossy(&fast_output.stderr)
    );
    let fast_output = fs::read_to_string(&arguments).unwrap();
    assert!(
        !fast_output.lines().any(|line| line == "--release"),
        "{fast_output}"
    );
    assert!(fast_output.contains("JOBS=2"), "{fast_output}");
    assert!(
        fast_output.contains("DEV_OPT=1\nINCREMENTAL=1"),
        "{fast_output}"
    );
    assert!(
        fast_output
            .lines()
            .any(|line| line == "mp3,sqlite,scripting,dev"),
        "{fast_output}"
    );
}
