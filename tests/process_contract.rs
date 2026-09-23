use bornengine_cli::package_manager::PackageManager;
use bornengine_cli::platform::{HostPlatform, PerryCapabilities, TargetRequest, resolve_target};
use bornengine_cli::process::{perry_check_args, perry_compile_args, perry_dev_args};
use std::path::Path;

#[test]
fn package_manager_aliases_parse_without_requiring_installation() {
    assert_eq!(PackageManager::parse("pnpm").unwrap(), PackageManager::Pnpm);
    assert_eq!(PackageManager::parse("npm").unwrap(), PackageManager::Npm);
    assert_eq!(PackageManager::parse("yarn").unwrap(), PackageManager::Yarn);
    assert_eq!(
        PackageManager::parse("PM").unwrap_err().to_string(),
        "unsupported package manager `PM`"
    );
}

#[test]
fn package_manager_install_commands_are_argument_arrays() {
    assert_eq!(PackageManager::Pnpm.install_args(), ["install"]);
    assert_eq!(PackageManager::Npm.install_args(), ["install"]);
    assert_eq!(PackageManager::Yarn.install_args(), ["install"]);
}

#[test]
fn compile_args_use_perrys_long_output_flag_and_target() {
    let target = resolve_target(
        &TargetRequest {
            os: Some("windows".to_owned()),
            target: None,
        },
        &PerryCapabilities::from_compile_help("Target platform: windows (default: native)"),
        HostPlatform::Linux,
    )
    .unwrap();
    let args = perry_compile_args(
        Path::new("main.ts"),
        Path::new(".bornengine/builds/my-game.exe"),
        &target,
        false,
    );

    assert_eq!(
        args,
        [
            "compile",
            "main.ts",
            "--output",
            ".bornengine/builds/my-game.exe",
            "--target",
            "windows"
        ]
    );
}

#[test]
fn macos_compile_uses_perrys_native_host_target() {
    let target = resolve_target(
        &TargetRequest {
            os: None,
            target: None,
        },
        &PerryCapabilities::default(),
        HostPlatform::MacOS,
    )
    .unwrap();
    let args = perry_compile_args(
        Path::new("main.ts"),
        Path::new(".bornengine/builds/game"),
        &target,
        false,
    );

    assert_eq!(
        args,
        ["compile", "main.ts", "--output", ".bornengine/builds/game"]
    );
}

#[test]
fn compile_args_preserve_an_exact_target_not_known_to_the_cli() {
    let target = resolve_target(
        &TargetRequest {
            os: None,
            target: Some("android-arm64".to_owned()),
        },
        &PerryCapabilities::from_compile_help("Target platform: android-arm64 (default: native)"),
        HostPlatform::Linux,
    )
    .unwrap();
    let args = perry_compile_args(
        Path::new("main.ts"),
        Path::new("build/game"),
        &target,
        false,
    );

    assert_eq!(
        args,
        [
            "compile",
            "main.ts",
            "--output",
            "build/game",
            "--target",
            "android-arm64"
        ]
    );
}

#[test]
fn check_and_dev_arguments_keep_entry_and_output_separate() {
    assert_eq!(
        perry_check_args(Path::new("src/main.ts"), Some("web"), false),
        ["check", "src/main.ts", "--target", "web"]
    );
    assert_eq!(
        perry_dev_args(
            Path::new("main.ts"),
            Path::new(".bornengine/builds/game"),
            false,
        ),
        ["dev", "main.ts", "--output", ".bornengine/builds/game"]
    );
}
