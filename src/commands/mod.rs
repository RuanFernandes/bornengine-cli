pub mod assets;
pub mod build;
pub mod config;
pub mod create;
pub mod diagnostics;
pub mod engine;
pub mod import;
pub mod project;
pub mod update;

use crate::cli::{Cli, Commands};
use anyhow::Result;

pub fn execute(cli: Cli) -> Result<i32> {
    let verbose = cli.verbose > 0;
    match cli.command {
        Commands::Create => create::create(verbose),
        Commands::New {
            project_name,
            package_manager,
            engine_version,
            engine_path,
            game_type,
        } => project::new(
            &project_name,
            package_manager,
            engine_version,
            engine_path,
            game_type,
            verbose,
        ),
        Commands::Init {
            package_manager,
            engine_version,
            engine_path,
            game_type,
        } => project::init(
            package_manager,
            engine_version,
            engine_path,
            game_type,
            verbose,
        ),
        Commands::Build {
            entry_file,
            name,
            os,
            target,
        } => build::build(
            &entry_file,
            name.as_deref(),
            os.as_deref(),
            target.as_deref(),
            verbose,
        ),
        Commands::Run {
            entry_file,
            name,
            os,
            target,
            program_args,
        } => build::run(
            &entry_file,
            name.as_deref(),
            os.as_deref(),
            target.as_deref(),
            &program_args,
            verbose,
        ),
        Commands::Dev {
            entry_file,
            name,
            os,
            target,
            watch,
        } => build::dev(
            &entry_file,
            name.as_deref(),
            os.as_deref(),
            target.as_deref(),
            watch,
            verbose,
        ),
        Commands::Clean => diagnostics::clean(verbose),
        Commands::Doctor => diagnostics::doctor(verbose),
        Commands::Info => diagnostics::info(),
        Commands::Version => diagnostics::version(),
        Commands::Engine { command } => engine::execute(command, verbose),
        Commands::Upgrade { version, latest } => {
            engine::upgrade(version.as_deref(), latest, verbose)
        }
        Commands::Update => update::check(),
        Commands::Config { command } => config::execute(command),
        Commands::Import { command } => match command {
            crate::cli::ImportCommands::Tiled { map_file, output } => {
                let project_root = std::env::current_dir()?;
                import::import_tiled(&map_file, &output, &project_root)?;
                println!("Imported {} to {}", map_file.display(), output.display());
                Ok(0)
            }
        },
        Commands::Assets { command } => match command {
            crate::cli::AssetCommands::Validate {
                project_root,
                json,
                orphan_policy,
                max_file_bytes,
                max_total_bytes,
                max_image_dimension,
                max_total_image_pixels,
            } => {
                let root = project_root.unwrap_or(std::env::current_dir()?);
                let options = assets::AssetValidationOptions {
                    orphan_policy,
                    max_file_bytes,
                    max_total_bytes,
                    max_image_dimension,
                    max_total_image_pixels,
                };
                let report = assets::validate_project_assets_with_options(&root, &options)?;
                if json {
                    println!("{}", serde_json::to_string(&report)?);
                } else {
                    for diagnostic in &report.diagnostics {
                        let severity = serde_json::to_value(diagnostic.severity)?;
                        let code = serde_json::to_value(diagnostic.code)?;
                        eprintln!(
                            "{} {} {}: {}",
                            severity.as_str().unwrap_or("error"),
                            code.as_str().unwrap_or("unknown"),
                            diagnostic.path,
                            diagnostic.message
                        );
                    }
                    println!(
                        "Validated {} project assets ({} bytes)",
                        report.summary.files, report.summary.bytes
                    );
                }
                Ok(i32::from(report.has_errors()))
            }
            crate::cli::AssetCommands::Pack {
                project_root,
                output,
            } => {
                let root = project_root.unwrap_or(std::env::current_dir()?);
                let summary = assets::pack_project(&root, &output)?;
                println!(
                    "Packed {} assets ({} bytes) to {}",
                    summary.files,
                    summary.bytes,
                    output.display()
                );
                Ok(0)
            }
        },
        Commands::Check {
            entry_file,
            os,
            target,
        } => build::check(&entry_file, os.as_deref(), target.as_deref(), verbose),
    }
}
