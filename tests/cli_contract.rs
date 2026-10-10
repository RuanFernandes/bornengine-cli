use bornengine_cli::cli::{Cli, Commands, CreateCommands};
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
        cli.command.unwrap(),
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
        cli.command.unwrap(),
        Commands::New {
            package_manager: Some(package_manager),
            engine_path: Some(path),
            ..
        } if package_manager.as_str() == "npm" && path.as_path() == Path::new("../BornEngine")
    ));
}

#[test]
fn new_accepts_each_native_game_profile() {
    for (kind, expected) in [
        ("2d", bornengine_cli::project::GameKind::TwoD),
        ("2.5d", bornengine_cli::project::GameKind::TwoPointFiveD),
        ("3d", bornengine_cli::project::GameKind::ThreeD),
    ] {
        let cli =
            Cli::try_parse_from(["bornengine", "new", "MyGame", "--game-type", kind]).unwrap();

        assert!(matches!(
            cli.command.unwrap(),
            Commands::New { game_type, .. } if game_type == expected
        ));
    }
}

#[test]
fn init_accepts_the_game_profile_and_defaults_to_two_dimensional() {
    let default = Cli::try_parse_from(["bornengine", "init"]).unwrap();
    assert!(matches!(
        default.command.unwrap(),
        Commands::Init {
            game_type: bornengine_cli::project::GameKind::TwoD,
            ..
        }
    ));

    let explicit = Cli::try_parse_from(["bornengine", "init", "--game-type", "3d"]).unwrap();
    assert!(matches!(
        explicit.command.unwrap(),
        Commands::Init {
            game_type: bornengine_cli::project::GameKind::ThreeD,
            ..
        }
    ));

    let optional = Cli::try_parse_from([
        "bornengine",
        "init",
        "--native-features",
        "sqlite,scripting",
    ])
    .unwrap();
    assert!(matches!(
        optional.command.unwrap(),
        Commands::Init { native_features, .. }
            if native_features == ["sqlite", "scripting"]
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
        cli.command.unwrap(),
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

    assert!(matches!(
        cli.command.unwrap(),
        Commands::Create { command: None }
    ));
}

#[test]
fn create_server_accepts_a_target_path_and_package_manager() {
    let parsed = Cli::try_parse_from([
        "bornengine",
        "create",
        "server",
        "backend",
        "--package-manager",
        "npm",
    ]);

    let cli = parsed.unwrap_or_else(|error| {
        panic!("`bornengine create server <path> --package-manager npm` should parse: {error}")
    });
    assert!(matches!(
        cli.command.unwrap(),
        Commands::Create {
            command: Some(CreateCommands::Server {
                path: Some(path),
                package_manager: Some(package_manager),
            })
        } if path == Path::new("backend") && package_manager.as_str() == "npm"
    ));
}

#[test]
fn create_help_keeps_the_wizard_and_documents_server_scaffolding() {
    let mut command = Cli::command();
    let create = command.find_subcommand_mut("create").unwrap();
    let create_help = create.render_long_help().to_string();
    assert!(create_help.contains("interactive prompts"));
    assert!(create_help.contains("server"));

    let server_help = create
        .find_subcommand_mut("server")
        .unwrap()
        .render_long_help()
        .to_string();
    assert!(server_help.contains("PATH"));
    assert!(server_help.contains("package-manager"));
}

#[test]
fn verbose_flag_is_available_after_a_subcommand() {
    let cli = Cli::try_parse_from(["bornengine", "doctor", "--verbose"]).unwrap();
    assert_eq!(cli.verbose, 1);
}

#[test]
fn check_exposes_perry_dependency_compatibility_options() {
    let cli = Cli::try_parse_from([
        "bornengine",
        "check",
        "src/main.ts",
        "--check-deps",
        "--deep-deps",
        "--all",
        "--strict",
    ])
    .unwrap();

    assert!(matches!(
        cli.command.unwrap(),
        Commands::Check {
            check_deps: true,
            deep_deps: true,
            show_all: true,
            strict: true,
            ..
        }
    ));

    let mut command = Cli::command();
    let help = command
        .find_subcommand_mut("check")
        .unwrap()
        .render_long_help()
        .to_string();
    assert!(help.contains("scan installed dependencies for compatibility"));
    assert!(help.contains("not only direct imports"));
    assert!(help.contains("all Perry findings, including hints"));
}

#[test]
fn deep_dependency_check_requires_dependency_scanning() {
    assert!(Cli::try_parse_from(["bornengine", "check", "src/main.ts", "--deep-deps",]).is_err());
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
        cli.command.unwrap(),
        Commands::Import { command } if matches!(&command,
            bornengine_cli::cli::ImportCommands::Tiled { map_file, output }
            if map_file.as_path() == Path::new("maps/level.tmx") && output.as_path() == Path::new("world/level.world2d.json"))
    ));
}

#[test]
fn asset_validate_and_pack_commands_accept_project_and_output() {
    let validate = Cli::try_parse_from(["bornengine", "assets", "validate", "game"]).unwrap();
    assert!(matches!(
        validate.command.unwrap(),
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
        pack.command.unwrap(),
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
        cli.command.unwrap(),
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
    assert!(help.contains("cache"), "{help}");
}

#[test]
fn cargo_cache_path_command_is_available_without_a_project() {
    let parsed = Cli::try_parse_from(["bornengine", "cache", "path"]);

    assert!(matches!(
        parsed.unwrap().command.unwrap(),
        Commands::Cache {
            command: bornengine_cli::cli::CacheCommands::Path
        }
    ));
}

#[test]
fn cargo_cache_warm_accepts_profile_and_job_options() {
    let parsed = Cli::try_parse_from(["bornengine", "cache", "warm", "--release", "--jobs", "3"]);

    assert!(matches!(
        parsed.unwrap().command.unwrap(),
        Commands::Cache {
            command: bornengine_cli::cli::CacheCommands::Warm {
                release: true,
                jobs: Some(3)
            }
        }
    ));
}

#[test]
fn native_build_commands_accept_jobs_and_fast_or_release_profiles() {
    let build = Cli::try_parse_from(["bornengine", "build", "main.ts", "--jobs", "4"]).unwrap();
    let run =
        Cli::try_parse_from(["bornengine", "run", "main.ts", "--release", "--jobs", "2"]).unwrap();
    let dev =
        Cli::try_parse_from(["bornengine", "dev", "main.ts", "--release", "--jobs", "3"]).unwrap();

    assert!(matches!(
        build.command.unwrap(),
        Commands::Build { jobs: Some(4), .. }
    ));
    assert!(matches!(
        run.command.unwrap(),
        Commands::Run {
            release: true,
            jobs: Some(2),
            ..
        }
    ));
    assert!(matches!(
        dev.command.unwrap(),
        Commands::Dev {
            release: true,
            jobs: Some(3),
            ..
        }
    ));
}

#[test]
fn native_build_commands_reject_non_positive_job_counts() {
    for args in [
        vec!["bornengine", "build", "main.ts", "--jobs=0"],
        vec!["bornengine", "run", "main.ts", "--jobs=-1"],
        vec!["bornengine", "dev", "main.ts", "--jobs=0"],
        vec!["bornengine", "cache", "warm", "--jobs=0"],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
}

#[test]
fn new_accepts_only_known_optional_native_features() {
    let parsed = Cli::try_parse_from([
        "bornengine",
        "new",
        "MyGame",
        "--native-features",
        "sqlite,scripting",
    ])
    .unwrap();
    assert!(matches!(
        parsed.command.unwrap(),
        Commands::New { native_features, .. }
            if native_features == ["sqlite", "scripting"]
    ));

    assert!(
        Cli::try_parse_from([
            "bornengine",
            "new",
            "MyGame",
            "--native-features",
            "rigid-body-magic",
        ])
        .is_err()
    );
}
