use bornengine_cli::cli::{Cli, Commands};
use clap::Parser;

#[test]
fn build_accepts_name_and_user_friendly_os() {
    let cli = Cli::try_parse_from([
        "bornengine",
        "build",
        "main.ts",
        "-n",
        "my-game",
        "-o",
        "linux",
    ])
    .unwrap();

    assert!(matches!(
        cli.command,
        Commands::Build {
            name: Some(name),
            os: Some(os),
            target: None,
            ..
        } if name == "my-game" && os == "linux"
    ));
}

#[test]
fn build_rejects_ambiguous_os_and_exact_target() {
    let result = Cli::try_parse_from([
        "bornengine",
        "build",
        "main.ts",
        "--os",
        "linux",
        "--target",
        "linux",
    ]);

    assert!(result.is_err());
}

#[test]
fn new_accepts_package_manager_and_local_engine_path() {
    let cli = Cli::try_parse_from([
        "bornengine",
        "new",
        "MyGame",
        "--package-manager",
        "npm",
        "--engine-path",
        "../BornEngine",
    ])
    .unwrap();

    assert!(matches!(
        cli.command,
        Commands::New {
            package_manager: Some(package_manager),
            engine_path: Some(path),
            ..
        } if package_manager.as_str() == "npm" && path == std::path::PathBuf::from("../BornEngine")
    ));
}

#[test]
fn verbose_flag_is_available_after_a_subcommand() {
    let cli = Cli::try_parse_from(["bornengine", "doctor", "--verbose"]).unwrap();
    assert_eq!(cli.verbose, 1);
}
