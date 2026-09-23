use crate::package_manager::PackageManager;
use clap::{ArgAction, Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "bornengine",
    version,
    about = "Create and manage BornEngine games"
)]
pub struct Cli {
    #[arg(short, long, global = true, action = ArgAction::Count, help = "Show detailed command output")]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Create a new BornEngine game project.
    New {
        project_name: String,
        #[arg(long, alias = "pm", value_enum)]
        package_manager: Option<PackageManager>,
        #[arg(short = 'e', long)]
        engine_version: Option<String>,
        #[arg(long)]
        engine_path: Option<PathBuf>,
    },
    /// Initialize a BornEngine game in the current directory.
    Init {
        #[arg(long, alias = "pm", value_enum)]
        package_manager: Option<PackageManager>,
        #[arg(short = 'e', long)]
        engine_version: Option<String>,
        #[arg(long)]
        engine_path: Option<PathBuf>,
    },
    /// Compile a game for a platform.
    Build {
        entry_file: PathBuf,
        #[arg(short = 'n', long)]
        name: Option<String>,
        #[arg(short = 'o', long, conflicts_with = "target")]
        os: Option<String>,
        #[arg(long, conflicts_with = "os")]
        target: Option<String>,
    },
    /// Compile and run a game for the current host.
    Run {
        entry_file: PathBuf,
        #[arg(short = 'n', long)]
        name: Option<String>,
        #[arg(short = 'o', long, conflicts_with = "target")]
        os: Option<String>,
        #[arg(long, conflicts_with = "os")]
        target: Option<String>,
        #[arg(last = true, allow_hyphen_values = true)]
        program_args: Vec<String>,
    },
    /// Build and run a game, optionally restarting it when files change.
    Dev {
        entry_file: PathBuf,
        #[arg(short = 'n', long)]
        name: Option<String>,
        #[arg(short = 'o', long, conflicts_with = "target")]
        os: Option<String>,
        #[arg(long, conflicts_with = "os")]
        target: Option<String>,
        #[arg(long, help = "Watch source files and restart after changes")]
        watch: bool,
    },
    /// Remove build files recorded by BornEngine CLI.
    Clean,
    /// Check the development environment.
    Doctor,
    /// Show information about the current project and toolchain.
    Info,
    /// Show the installed CLI version.
    Version,
    /// Manage the BornEngine dependency of the current project.
    Engine {
        #[command(subcommand)]
        command: EngineCommands,
    },
    /// Upgrade the engine dependency in the current project.
    Upgrade {
        version: Option<String>,
        #[arg(long, conflicts_with = "version")]
        latest: bool,
    },
    /// Check how to install a newer CLI release.
    Update,
    /// Read or update global CLI configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Check TypeScript compatibility without creating a binary.
    Check {
        entry_file: PathBuf,
        #[arg(short = 'o', long, conflicts_with = "target")]
        os: Option<String>,
        #[arg(long, conflicts_with = "os")]
        target: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum EngineCommands {
    /// Show the engine dependency selected by this project.
    Current,
    /// Install a specific engine version, or the configured latest version.
    Install { version: Option<String> },
    /// List stable engine releases available from the registry.
    List,
    /// Upgrade the current engine dependency to the latest stable release.
    Update,
    /// Remove the engine dependency from this project.
    Remove { version: Option<String> },
    /// Select an engine version or local checkout for this project.
    Use { source: String },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    /// Set a global configuration value.
    Set { key: String, value: String },
    /// Read a global configuration value.
    Get { key: String },
    /// List all global configuration values.
    List,
}

#[derive(Debug, Args)]
pub struct ProjectOptions {
    #[arg(long, alias = "pm", value_enum)]
    pub package_manager: Option<PackageManager>,
    #[arg(short = 'e', long)]
    pub engine_version: Option<String>,
    #[arg(long)]
    pub engine_path: Option<PathBuf>,
}
