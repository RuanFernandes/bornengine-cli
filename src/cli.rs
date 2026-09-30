use crate::package_manager::PackageManager;
use clap::{ArgAction, Parser, Subcommand};
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
    /// Create a BornEngine game project with interactive prompts.
    Create,
    /// Create a new BornEngine game project.
    New {
        #[arg(help = "Name of the new project directory")]
        project_name: String,
        #[arg(
            long,
            alias = "pm",
            value_enum,
            help = "Package manager to install dependencies with (default: pnpm)"
        )]
        package_manager: Option<PackageManager>,
        #[arg(
            short = 'e',
            long,
            help = "Exact stable BornEngine version (default: latest)"
        )]
        engine_version: Option<String>,
        #[arg(long, alias = "engine", help = "Path to a local BornEngine checkout")]
        engine_path: Option<PathBuf>,
    },
    /// Initialize a BornEngine game in the current directory.
    Init {
        #[arg(
            long,
            alias = "pm",
            value_enum,
            help = "Package manager to install dependencies with (default: pnpm)"
        )]
        package_manager: Option<PackageManager>,
        #[arg(
            short = 'e',
            long,
            help = "Exact stable BornEngine version (default: latest)"
        )]
        engine_version: Option<String>,
        #[arg(long, alias = "engine", help = "Path to a local BornEngine checkout")]
        engine_path: Option<PathBuf>,
    },
    /// Compile a game for a platform.
    Build {
        #[arg(help = "TypeScript entry file")]
        entry_file: PathBuf,
        #[arg(short = 'n', long, help = "Output file name")]
        name: Option<String>,
        #[arg(
            short = 'o',
            long,
            conflicts_with = "target",
            help = "Friendly OS target (defaults to the current host)"
        )]
        os: Option<String>,
        #[arg(
            long,
            conflicts_with = "os",
            help = "Exact target name advertised by Perry"
        )]
        target: Option<String>,
    },
    /// Compile and run a game for the current host.
    Run {
        #[arg(help = "TypeScript entry file")]
        entry_file: PathBuf,
        #[arg(short = 'n', long, help = "Output file name")]
        name: Option<String>,
        #[arg(
            short = 'o',
            long,
            conflicts_with = "target",
            help = "Friendly OS target (defaults to the current host)"
        )]
        os: Option<String>,
        #[arg(
            long,
            conflicts_with = "os",
            help = "Exact target name advertised by Perry"
        )]
        target: Option<String>,
        #[arg(last = true, allow_hyphen_values = true)]
        program_args: Vec<String>,
    },
    /// Build and run a game, optionally restarting it when files change.
    Dev {
        #[arg(help = "TypeScript entry file")]
        entry_file: PathBuf,
        #[arg(short = 'n', long, help = "Output file name")]
        name: Option<String>,
        #[arg(
            short = 'o',
            long,
            conflicts_with = "target",
            help = "Friendly OS target (defaults to the current host)"
        )]
        os: Option<String>,
        #[arg(
            long,
            conflicts_with = "os",
            help = "Exact target name advertised by Perry"
        )]
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
    /// Import maps from external authoring tools.
    Import {
        #[command(subcommand)]
        command: ImportCommands,
    },
    /// Validate or package project assets.
    Assets {
        #[command(subcommand)]
        command: AssetCommands,
    },
    /// Validate or package a self-contained JavaScript behavior.
    Script {
        #[command(subcommand)]
        command: ScriptCommands,
    },
    /// Check TypeScript compatibility without creating a binary.
    Check {
        #[arg(help = "TypeScript entry file")]
        entry_file: PathBuf,
        #[arg(
            short = 'o',
            long,
            conflicts_with = "target",
            help = "Friendly OS target (defaults to the current host)"
        )]
        os: Option<String>,
        #[arg(
            long,
            conflicts_with = "os",
            help = "Exact target name advertised by Perry"
        )]
        target: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ImportCommands {
    /// Convert a Tiled orthogonal TMX map to BornEngine world2d JSON.
    Tiled {
        #[arg(help = "Tiled TMX map file")]
        map_file: PathBuf,
        #[arg(long, required = true, help = "Output .world2d.json file")]
        output: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum AssetCommands {
    /// Check project asset references and paths.
    Validate {
        #[arg(help = "Project root (defaults to the current directory)")]
        project_root: Option<PathBuf>,
        #[arg(long, help = "Write a versioned JSON validation report")]
        json: bool,
        #[arg(long, value_enum, help = "How to report unreferenced assets")]
        orphan_policy: Option<crate::commands::assets::OrphanPolicy>,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..), help = "Maximum bytes in one asset")]
        max_file_bytes: Option<u64>,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..), help = "Maximum combined asset bytes")]
        max_total_bytes: Option<u64>,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..), help = "Maximum width or height of an image")]
        max_image_dimension: Option<u32>,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..), help = "Maximum combined image pixels")]
        max_total_image_pixels: Option<u64>,
    },
    /// Copy project assets and write a deterministic manifest.
    Pack {
        #[arg(help = "Project root (defaults to the current directory)")]
        project_root: Option<PathBuf>,
        #[arg(long, required = true, help = "Directory where assets will be copied")]
        output: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum ScriptCommands {
    /// Validate a BornEngine JavaScript behavior package.
    Check {
        #[arg(
            long,
            help = "Path to bornengine.script.json (defaults to the current directory)"
        )]
        manifest: Option<PathBuf>,
    },
    /// Package one validated JavaScript behavior for runtime loading.
    Pack {
        #[arg(
            long,
            required = true,
            help = "Directory where the script package will be written"
        )]
        output: PathBuf,
        #[arg(
            long,
            help = "Path to bornengine.script.json (defaults to the current directory)"
        )]
        manifest: Option<PathBuf>,
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
