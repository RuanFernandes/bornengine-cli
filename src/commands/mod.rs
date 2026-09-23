pub mod build;
pub mod config;
pub mod diagnostics;
pub mod engine;
pub mod project;
pub mod update;

use crate::cli::{Cli, Commands};
use anyhow::Result;

pub fn execute(cli: Cli) -> Result<i32> {
    let verbose = cli.verbose > 0;
    match cli.command {
        Commands::New {
            project_name,
            package_manager,
            engine_version,
            engine_path,
        } => project::new(
            &project_name,
            package_manager,
            engine_version,
            engine_path,
            verbose,
        ),
        Commands::Init {
            package_manager,
            engine_version,
            engine_path,
        } => project::init(package_manager, engine_version, engine_path, verbose),
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
        Commands::Check {
            entry_file,
            os,
            target,
        } => build::check(&entry_file, os.as_deref(), target.as_deref(), verbose),
    }
}
