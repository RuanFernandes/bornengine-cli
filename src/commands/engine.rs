use crate::cli::EngineCommands;
use crate::config::Config;
use crate::engine::{
    EngineDependency, engine_dependency, list_engine_releases, local_engine_dependency,
    remove_engine_dependency, resolve_engine_release, set_engine_dependency,
};
use crate::package_manager::PackageManager;
use crate::process::{executable_in_path, inherited_command};
use crate::project::{find_project_root, read_package_json, write_package_json};
use crate::ui::{self, Tone};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn execute(command: EngineCommands, verbose: bool) -> Result<i32> {
    match command {
        EngineCommands::Current => current(),
        EngineCommands::Install { version } => upgrade(version.as_deref(), false, verbose),
        EngineCommands::List => list(),
        EngineCommands::Update => upgrade(None, true, verbose),
        EngineCommands::Remove { version } => remove(version.as_deref(), verbose),
        EngineCommands::Use { source } => use_source(&source, verbose),
    }
}

pub fn upgrade(version: Option<&str>, latest: bool, verbose: bool) -> Result<i32> {
    let root = current_project_root()?;
    let config = Config::load_default()?;
    let manager = project_package_manager(&root, &config)?;
    ensure_manager(manager)?;
    let requested = if latest {
        "latest"
    } else {
        version.unwrap_or("latest")
    };
    let release = resolve_engine_release(Some(requested))?;
    let dependency = EngineDependency {
        package_name: release.package_name,
        spec: release.version,
    };
    set_dependency(&root, manager, dependency, verbose)
}

pub fn current() -> Result<i32> {
    let root = current_project_root()?;
    let package = read_package_json(&root)?;
    let dependency = engine_dependency(&package)?
        .context("this project does not declare a BornEngine dependency")?;
    println!(
        "{} {}",
        ui::paint(&dependency.package_name, Tone::Accent),
        dependency.spec
    );
    println!("Source: {}", dependency_source(&dependency.spec));
    Ok(0)
}

pub fn list() -> Result<i32> {
    for release in list_engine_releases()? {
        println!(
            "{} {}",
            ui::paint(release.version, Tone::Accent),
            release.package_name
        );
    }
    Ok(0)
}

pub fn use_source(source: &str, verbose: bool) -> Result<i32> {
    let root = current_project_root()?;
    let config = Config::load_default()?;
    let manager = project_package_manager(&root, &config)?;
    ensure_manager(manager)?;
    let dependency = if Path::new(source).is_dir() {
        local_engine_dependency(Path::new(source), &root, manager)?
    } else {
        let release = resolve_engine_release(Some(source))?;
        EngineDependency {
            package_name: release.package_name,
            spec: release.version,
        }
    };
    set_dependency(&root, manager, dependency, verbose)
}

fn remove(version: Option<&str>, verbose: bool) -> Result<i32> {
    let root = current_project_root()?;
    let config = Config::load_default()?;
    let manager = project_package_manager(&root, &config)?;
    ensure_manager(manager)?;
    let mut package = read_package_json(&root)?;
    let Some(current) = engine_dependency(&package)? else {
        println!(
            "{}",
            ui::paint("This project has no BornEngine dependency.", Tone::Info)
        );
        return Ok(0);
    };
    if let Some(version) = version {
        if version != current.spec {
            bail!(
                "the current BornEngine dependency is `{}`; refusing to remove `{version}`",
                current.spec
            );
        }
    }
    remove_engine_dependency(&mut package)?;
    write_package_json(&root, &package)?;
    println!(
        "{}",
        ui::paint(
            format!("Removed {} {}", current.package_name, current.spec),
            Tone::Success
        )
    );
    install(manager, &root, verbose)
}

fn set_dependency(
    root: &Path,
    manager: PackageManager,
    dependency: EngineDependency,
    verbose: bool,
) -> Result<i32> {
    let mut package = read_package_json(root)?;
    let previous = engine_dependency(&package)?;
    if previous.as_ref() == Some(&dependency) {
        println!(
            "{}",
            ui::paint(
                format!(
                    "BornEngine dependency is already {} {}",
                    dependency.package_name, dependency.spec
                ),
                Tone::Info
            )
        );
        return Ok(0);
    }
    set_engine_dependency(&mut package, &dependency)?;
    write_package_json(root, &package)?;
    if let Some(previous) = previous {
        println!(
            "{}",
            ui::paint(
                format!(
                    "Updated BornEngine: {} {} -> {} {}",
                    previous.package_name, previous.spec, dependency.package_name, dependency.spec
                ),
                Tone::Success
            )
        );
    } else {
        println!(
            "{}",
            ui::paint(
                format!(
                    "Added BornEngine: {} {}",
                    dependency.package_name, dependency.spec
                ),
                Tone::Success
            )
        );
    }
    install(manager, root, verbose)
}

fn install(manager: PackageManager, root: &Path, verbose: bool) -> Result<i32> {
    let args = manager
        .install_args()
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    inherited_command(manager.executable(), &args, Some(root), verbose)
}

fn current_project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    find_project_root(&cwd)?
        .context("could not find a BornEngine project in this directory or its parents")
}

fn project_package_manager(root: &Path, config: &Config) -> Result<PackageManager> {
    let package = read_package_json(root)?;
    if let Some(value) = package.get("packageManager").and_then(Value::as_str) {
        let name = value.split('@').next().unwrap_or(value);
        return PackageManager::parse(name);
    }
    for (lockfile, manager) in [
        ("pnpm-lock.yaml", PackageManager::Pnpm),
        ("package-lock.json", PackageManager::Npm),
        ("yarn.lock", PackageManager::Yarn),
    ] {
        if root.join(lockfile).is_file() {
            return Ok(manager);
        }
    }
    PackageManager::parse(&config.package_manager)
}

fn ensure_manager(manager: PackageManager) -> Result<()> {
    if executable_in_path(manager.executable()) {
        return Ok(());
    }
    let instruction = match manager {
        PackageManager::Pnpm => "install Node.js, then run `npm install --global pnpm`",
        PackageManager::Npm => "install Node.js from https://nodejs.org/",
        PackageManager::Yarn => "install Node.js, then run `corepack enable`",
    };
    bail!(
        "package manager `{}` was not found in PATH; {instruction}",
        manager.as_str()
    );
}

fn dependency_source(spec: &str) -> &'static str {
    if spec.starts_with("link:") || spec.starts_with("file:") {
        "local path"
    } else if spec.starts_with("github:") || spec.starts_with("git+") {
        "GitHub/Git"
    } else {
        "npm registry"
    }
}
