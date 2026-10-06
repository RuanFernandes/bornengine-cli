use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn write_bornengine_project(root: &Path) {
    fs::write(
        root.join("package.json"),
        r#"{"name":"test-game","dependencies":{"@bornengine/engine":"0.5.0"}}"#,
    )
    .unwrap();
}

fn run_cli(root: &Path, args: &[&str], path: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_bornengine"));
    command.args(args).current_dir(root);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command.output().unwrap()
}

#[cfg(unix)]
struct FakeManager {
    path: PathBuf,
    args_file: PathBuf,
}

#[cfg(unix)]
fn fake_manager(root: &Path, manager: &str, exit_code: i32) -> FakeManager {
    use std::os::unix::fs::PermissionsExt;

    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let script = bin.join(manager);
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$BORNENGINE_TEST_ARGS_FILE\"\nlast=''\nfor arg in \"$@\"; do last=\"$arg\"; done\nmkdir -p \"$last\"\nprintf '{{}}\\n' > \"$last/package.json\"\nexit {exit_code}\n"
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).unwrap();
    let args_file = root.join("generator-args.txt");
    let existing_path = std::env::var_os("PATH").unwrap_or_default();
    let search_path = std::env::join_paths(
        std::iter::once(bin.as_os_str().to_owned())
            .chain(std::env::split_paths(&existing_path).map(PathBuf::into_os_string)),
    )
    .unwrap();

    FakeManager {
        path: search_path.into(),
        args_file,
    }
}

#[cfg(unix)]
#[test]
fn create_server_uses_the_selected_colyseus_generator_and_writes_the_marker_after_success() {
    for (manager, generator_name) in [
        ("npm", "colyseus-app@latest"),
        ("pnpm", "colyseus-app@latest"),
        ("yarn", "colyseus-app"),
    ] {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write_bornengine_project(root);
        let fake = fake_manager(root, manager, 0);
        let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
            .args(["create", "server", "--package-manager", manager])
            .current_dir(root)
            .env("PATH", &fake.path)
            .env("BORNENGINE_TEST_ARGS_FILE", &fake.args_file)
            .output()
            .unwrap();

        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let target = root.join("server");
        let actual_args = fs::read_to_string(&fake.args_file).unwrap();
        assert_eq!(
            actual_args.lines().collect::<Vec<_>>(),
            ["create", generator_name, target.to_str().unwrap()],
            "wrong generator arguments for {manager}"
        );
        let marker: serde_json::Value =
            serde_json::from_slice(&fs::read(target.join("bornengine.server.json")).unwrap())
                .unwrap();
        assert_eq!(marker["format"], "bornengine.server");
        assert_eq!(marker["version"], 1);
        assert_eq!(marker["provider"], "colyseus");
        assert_eq!(marker["clientProjectRoot"], "..");
    }
}

#[cfg(unix)]
#[test]
fn create_server_propagates_generator_failure_without_claiming_the_server() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_bornengine_project(root);
    let fake = fake_manager(root, "npm", 7);
    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args(["create", "server", "--package-manager", "npm"])
        .current_dir(root)
        .env("PATH", &fake.path)
        .env("BORNENGINE_TEST_ARGS_FILE", &fake.args_file)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(7));
    assert!(fake.args_file.exists());
    assert!(!root.join("server/bornengine.server.json").exists());
}

#[cfg(unix)]
#[test]
fn create_server_refuses_a_non_empty_target_without_modifying_it() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_bornengine_project(root);
    let target = root.join("server");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep.txt"), "owned by the project").unwrap();
    let fake = fake_manager(root, "npm", 0);
    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args(["create", "server", "--package-manager", "npm"])
        .current_dir(root)
        .env("PATH", &fake.path)
        .env("BORNENGINE_TEST_ARGS_FILE", &fake.args_file)
        .output()
        .unwrap();

    assert_ne!(output.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(target.join("keep.txt")).unwrap(),
        "owned by the project"
    );
    assert!(!fake.args_file.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not empty"));
}

#[cfg(unix)]
#[test]
fn create_server_accepts_a_nested_target_and_records_the_relative_client_root() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_bornengine_project(root);
    let fake = fake_manager(root, "npm", 0);
    let target = root.join("services/arena");
    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args([
            "create",
            "server",
            "services/arena",
            "--package-manager",
            "npm",
        ])
        .current_dir(root)
        .env("PATH", &fake.path)
        .env("BORNENGINE_TEST_ARGS_FILE", &fake.args_file)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual_args = fs::read_to_string(&fake.args_file).unwrap();
    assert_eq!(
        actual_args.lines().collect::<Vec<_>>(),
        ["create", "colyseus-app@latest", target.to_str().unwrap()]
    );
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(target.join("bornengine.server.json")).unwrap()).unwrap();
    assert_eq!(marker["clientProjectRoot"], "../..");
}

#[cfg(unix)]
#[test]
fn create_server_accepts_an_absolute_target_inside_the_project() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_bornengine_project(root);
    let fake = fake_manager(root, "npm", 0);
    let target = root.join("backend");
    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args([
            "create",
            "server",
            target.to_str().unwrap(),
            "--package-manager",
            "npm",
        ])
        .current_dir(root)
        .env("PATH", &fake.path)
        .env("BORNENGINE_TEST_ARGS_FILE", &fake.args_file)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(target.join("bornengine.server.json")).unwrap()).unwrap();
    assert_eq!(marker["clientProjectRoot"], "..");
}

#[cfg(unix)]
#[test]
fn create_server_rejects_a_target_outside_the_bornengine_project() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("game");
    fs::create_dir(&root).unwrap();
    write_bornengine_project(&root);
    let fake = fake_manager(temp.path(), "npm", 0);
    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args([
            "create",
            "server",
            "../outside-server",
            "--package-manager",
            "npm",
        ])
        .current_dir(&root)
        .env("PATH", &fake.path)
        .env("BORNENGINE_TEST_ARGS_FILE", &fake.args_file)
        .output()
        .unwrap();

    assert_ne!(output.status.code(), Some(0));
    assert!(!fake.args_file.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("inside the BornEngine project"));
}

#[cfg(unix)]
#[test]
fn create_server_uses_the_configured_package_manager_when_no_override_is_given() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_bornengine_project(root);
    let fake = fake_manager(root, "yarn", 0);
    let config_home = root.join("config");
    let config_dir = config_home.join("bornengine");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.toml"),
        "package_manager = \"yarn\"\nengine_version = \"latest\"\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args(["create", "server"])
        .current_dir(root)
        .env("PATH", &fake.path)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("BORNENGINE_TEST_ARGS_FILE", &fake.args_file)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual_args = fs::read_to_string(&fake.args_file).unwrap();
    assert_eq!(
        actual_args.lines().collect::<Vec<_>>(),
        [
            "create",
            "colyseus-app",
            root.join("server").to_str().unwrap()
        ]
    );
}

#[test]
fn create_server_requires_a_bornengine_project() {
    let temp = TempDir::new().unwrap();
    let output = run_cli(
        temp.path(),
        &["create", "server", "--package-manager", "npm"],
        None,
    );

    assert_ne!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).contains("BornEngine project"));
}
