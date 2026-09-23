use crate::config::Config;
use crate::engine::{local_engine_dependency, resolve_engine_release};
use crate::package_manager::PackageManager;
use crate::process::{executable_in_path, inherited_command};
use crate::project::{ProjectSpec, create_project, initialize_project, validate_project_name};
use crate::ui::{self, Tone};
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn new(
    project_name: &str,
    package_manager: Option<PackageManager>,
    engine_version: Option<String>,
    engine_path: Option<PathBuf>,
    verbose: bool,
) -> Result<i32> {
    validate_project_name(project_name)?;
    let parent = std::env::current_dir()
        .context("could not determine current directory")?
        .canonicalize()
        .context("could not resolve current directory")?;
    let config = Config::load_default()?;
    let manager = select_package_manager(package_manager, &config)?;
    ensure_manager(manager)?;
    let root = parent.join(project_name);
    let spec = resolve_project_spec(
        manager,
        engine_version.as_deref(),
        engine_path.as_deref(),
        &root,
        &config,
    )?;

    println!(
        "{}",
        ui::paint(
            format!("Creating BornEngine project: {project_name}"),
            Tone::Heading
        )
    );
    let project_root = create_project(&parent, project_name, &spec)?;
    install_dependencies(manager, &project_root, verbose)
}

pub fn init(
    package_manager: Option<PackageManager>,
    engine_version: Option<String>,
    engine_path: Option<PathBuf>,
    verbose: bool,
) -> Result<i32> {
    let root = std::env::current_dir()
        .context("could not determine current directory")?
        .canonicalize()
        .context("could not resolve current directory")?;
    let project_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .context("current directory does not have a valid project name")?;
    validate_project_name(project_name)?;
    let config = Config::load_default()?;
    let manager = select_package_manager(package_manager, &config)?;
    ensure_manager(manager)?;
    let spec = resolve_project_spec(
        manager,
        engine_version.as_deref(),
        engine_path.as_deref(),
        &root,
        &config,
    )?;

    println!(
        "{}",
        ui::paint(
            format!("Initializing BornEngine project in {}", root.display()),
            Tone::Heading
        )
    );
    initialize_project(&root, project_name, &spec)?;
    install_dependencies(manager, &root, verbose)
}

pub fn select_package_manager(
    explicit: Option<PackageManager>,
    config: &Config,
) -> Result<PackageManager> {
    match explicit {
        Some(manager) => Ok(manager),
        None => PackageManager::parse(&config.package_manager),
    }
}

pub fn ensure_manager(manager: PackageManager) -> Result<()> {
    if executable_in_path(manager.executable()) {
        return Ok(());
    }
    let instructions = match manager {
        PackageManager::Pnpm => "install Node.js, then run `npm install --global pnpm`",
        PackageManager::Npm => "install Node.js from https://nodejs.org/",
        PackageManager::Yarn => "install Node.js, then run `corepack enable`",
    };
    bail!(
        "package manager `{}` was not found in PATH; {instructions}",
        manager.as_str()
    );
}

pub fn resolve_project_spec(
    manager: PackageManager,
    explicit_version: Option<&str>,
    explicit_path: Option<&Path>,
    project_root: &Path,
    config: &Config,
) -> Result<ProjectSpec> {
    let environment_path = std::env::var_os("BORNENGINE_PATH").map(PathBuf::from);
    if let Some(path) = explicit_path.or(environment_path.as_deref()) {
        let dependency = local_engine_dependency(path, project_root, manager)?;
        return Ok(ProjectSpec {
            engine_package: dependency.package_name,
            engine_spec: dependency.spec,
        });
    }
    let requested = explicit_version.unwrap_or(&config.engine_version);
    let release = resolve_engine_release(Some(requested))?;
    Ok(ProjectSpec {
        engine_package: release.package_name,
        engine_spec: release.version,
    })
}

pub fn install_dependencies(
    manager: PackageManager,
    project_root: &Path,
    verbose: bool,
) -> Result<i32> {
    println!(
        "{}",
        ui::paint(
            format!("Installing dependencies with {}...", manager.as_str()),
            Tone::Info
        )
    );
    let args = manager
        .install_args()
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let exit_code = inherited_command(manager.executable(), &args, Some(project_root), verbose)?;
    if exit_code == 0 {
        println!(
            "{}\n\n  cd {}\n  bornengine run main.ts",
            ui::paint("Project created successfully.", Tone::Success),
            display_path(project_root)
        );
    } else {
        eprintln!(
            "{} Project files are in {}; dependency installation failed. Retry with `{} install`.",
            ui::paint_stderr("Warning:", Tone::Warning),
            project_root.display(),
            manager.as_str()
        );
    }
    Ok(exit_code)
}

fn display_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| ".".to_owned())
}
