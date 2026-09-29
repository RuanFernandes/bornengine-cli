use bornengine_cli::cli::{Cli, Commands};
use clap::{CommandFactory, Parser};
use std::path::Path;

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
        } if package_manager.as_str() == "npm" && path.as_path() == Path::new("../BornEngine")
    ));
}

#[test]
fn new_accepts_short_package_manager_and_engine_aliases() {
    let cli = Cli::try_parse_from([
        "bornengine",
        "new",
        "MyGame",
        "--pm",
        "yarn",
        "--engine",
        "../BornEngine",
    ])
    .unwrap();

    assert!(matches!(
        cli.command,
        Commands::New {
            package_manager: Some(package_manager),
            engine_path: Some(path),
            ..
        } if package_manager.as_str() == "yarn" && path.as_path() == Path::new("../BornEngine")
    ));
}

#[test]
fn create_is_available_as_a_separate_interactive_command() {
    let cli = Cli::try_parse_from(["bornengine", "create"]).unwrap();

    assert!(matches!(cli.command, Commands::Create));
}

#[test]
fn verbose_flag_is_available_after_a_subcommand() {
    let cli = Cli::try_parse_from(["bornengine", "doctor", "--verbose"]).unwrap();
    assert_eq!(cli.verbose, 1);
}

#[test]
fn tiled_import_command_accepts_map_and_output_paths() {
    let cli = Cli::try_parse_from([
        "bornengine",
        "import",
        "tiled",
        "maps/level.tmx",
        "--output",
        "world/level.world2d.json",
    ])
    .unwrap();

    assert!(matches!(
        cli.command,
        Commands::Import { command } if matches!(&command,
            bornengine_cli::cli::ImportCommands::Tiled { map_file, output }
            if map_file.as_path() == Path::new("maps/level.tmx") && output.as_path() == Path::new("world/level.world2d.json"))
    ));
}

#[test]
fn asset_validate_and_pack_commands_accept_project_and_output() {
    let validate = Cli::try_parse_from(["bornengine", "assets", "validate", "game"]).unwrap();
    assert!(matches!(
        validate.command,
        Commands::Assets { command: bornengine_cli::cli::AssetCommands::Validate { project_root: Some(path), .. } }
            if path == Path::new("game")
    ));

    let pack = Cli::try_parse_from([
        "bornengine",
        "assets",
        "pack",
        "game",
        "--output",
        "game/.bornengine/assets",
    ])
    .unwrap();
    assert!(matches!(
        pack.command,
        Commands::Assets { command: bornengine_cli::cli::AssetCommands::Pack { project_root: Some(root), output } }
            if root == Path::new("game") && output == Path::new("game/.bornengine/assets")
    ));
}

#[test]
fn asset_validate_parses_report_flags() {
    let cli = Cli::try_parse_from([
        "bornengine",
        "assets",
        "validate",
        "game",
        "--json",
        "--orphan-policy",
        "error",
        "--max-file-bytes",
        "10",
        "--max-total-bytes",
        "20",
        "--max-image-dimension",
        "32",
        "--max-total-image-pixels",
        "1024",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Commands::Assets {
            command: bornengine_cli::cli::AssetCommands::Validate {
                json: true,
                orphan_policy: Some(bornengine_cli::commands::assets::OrphanPolicy::Error),
                max_file_bytes: Some(10),
                max_total_bytes: Some(20),
                max_image_dimension: Some(32),
                max_total_image_pixels: Some(1024),
                ..
            }
        }
    ));
}

#[test]
fn asset_validate_rejects_zero_limits() {
    for flag in [
        "--max-file-bytes",
        "--max-total-bytes",
        "--max-image-dimension",
        "--max-total-image-pixels",
    ] {
        let result = Cli::try_parse_from(["bornengine", "assets", "validate", flag, "0"]);
        assert!(result.is_err(), "{flag} accepted zero");
    }
}

#[test]
fn top_level_help_lists_import_and_asset_management_commands() {
    let help = Cli::command().render_long_help().to_string();
    assert!(help.contains("import"), "{help}");
    assert!(help.contains("assets"), "{help}");
}
