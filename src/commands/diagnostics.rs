use crate::build_artifacts::clean_build_artifacts;
use crate::config::Config;
use crate::engine::engine_dependency;
use crate::engine_package::native_library_allow_pattern;
use crate::package_manager::PackageManager;
use crate::platform::{HostPlatform, PerryCapabilities, TargetRequest, resolve_target};
use crate::process::{captured_command, executable_in_path};
use crate::project::{find_project_root, project_name, read_package_json};
use crate::ui::{self, Tone};
use anyhow::{Context, Result};
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn clean(verbose: bool) -> Result<i32> {
    let root = current_project_root()?;
    let removed = clean_build_artifacts(&root)?;
    if removed == 0 {
        println!(
            "{}",
            ui::paint("No BornEngine build artifacts were recorded.", Tone::Info)
        );
    } else {
        println!(
            "{}",
            ui::paint(
                format!("Removed {removed} BornEngine build artifact(s)."),
                Tone::Success
            )
        );
    }
    if verbose {
        println!("Project: {}", root.display());
    }
    Ok(0)
}

pub fn doctor(verbose: bool) -> Result<i32> {
    println!("{}\n", ui::paint("BornEngine Doctor", Tone::Heading));
    let mut healthy = true;
    let perry_version = check_version("Perry", "perry", "--version", &mut healthy);
    check_version("Rust", "rustc", "--version", &mut healthy);
    check_version("Cargo", "cargo", "--version", &mut healthy);

    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let root = find_project_root(&cwd)?;
    let config = Config::load_default()?;
    let manager = match root.as_deref() {
        Some(root) => package_manager_for_project(root, &config)?,
        None => PackageManager::parse(&config.package_manager)?,
    };
    if executable_in_path(manager.executable()) {
        println!("{} {}", ui::paint("[OK]", Tone::Success), manager.as_str());
    } else {
        println!(
            "{} {} was not found in PATH",
            ui::paint("[FAIL]", Tone::Error),
            manager.as_str()
        );
        println!(
            "      Install Node.js and run `npm install --global {}`.",
            manager.as_str()
        );
        healthy = false;
    }

    if let Some(version) = perry_version {
        match compile_capabilities(verbose) {
            Ok(capabilities) => match resolve_target(
                &TargetRequest::default(),
                &capabilities,
                HostPlatform::current(),
            ) {
                Ok(target) => println!(
                    "{} {} target",
                    ui::paint("[OK]", Tone::Success),
                    target.perry_target.as_deref().unwrap_or("native")
                ),
                Err(error) => {
                    println!(
                        "{} Perry cannot build for this host: {error}",
                        ui::paint("[FAIL]", Tone::Error)
                    );
                    healthy = false;
                }
            },
            Err(error) => {
                println!(
                    "{} Could not inspect Perry targets: {error}",
                    ui::paint("[FAIL]", Tone::Error)
                );
                healthy = false;
            }
        }
        if verbose {
            println!("Perry version: {version}");
        }
    }

    if HostPlatform::current() == HostPlatform::Linux {
        healthy &= check_linux_prerequisites();
    }

    match root {
        Some(root) => {
            println!("Project: {}", root.display());
            let package = read_package_json(&root)?;
            if let Some(name) = project_name(&root)? {
                println!(
                    "{} Project metadata: {name}",
                    ui::paint("[OK]", Tone::Success)
                );
            } else {
                println!(
                    "{} package.json does not define a project name",
                    ui::paint("[WARN]", Tone::Warning)
                );
            }
            if let Some(dependency) = engine_dependency(&package)? {
                println!(
                    "{} Engine dependency: {} {} ({})",
                    ui::paint("[OK]", Tone::Success),
                    dependency.package_name,
                    dependency.spec,
                    dependency_source(&dependency.spec)
                );
                let installed = root
                    .join("node_modules")
                    .join(&dependency.package_name)
                    .join("package.json")
                    .is_file();
                if installed {
                    println!(
                        "{} BornEngine package is installed",
                        ui::paint("[OK]", Tone::Success)
                    );
                } else {
                    println!(
                        "{} BornEngine package is missing from node_modules; run `{}`",
                        ui::paint("[FAIL]", Tone::Error),
                        manager.install_args().join(" ")
                    );
                    healthy = false;
                }
                let required_pattern = native_library_allow_pattern(&dependency.package_name);
                let allowed = package
                    .pointer("/perry/allow/nativeLibrary")
                    .and_then(Value::as_array)
                    .is_some_and(|entries| {
                        entries
                            .iter()
                            .any(|entry| entry.as_str() == Some(&required_pattern))
                    });
                if allowed {
                    println!(
                        "{} Perry native-library allowlist",
                        ui::paint("[OK]", Tone::Success)
                    );
                } else {
                    println!(
                        "{} Perry allowlist is missing `{required_pattern}`",
                        ui::paint("[FAIL]", Tone::Error)
                    );
                    healthy = false;
                }
            } else {
                println!(
                    "{} No BornEngine dependency is declared in package.json",
                    ui::paint("[FAIL]", Tone::Error)
                );
                healthy = false;
            }
            if root.join("main.ts").is_file() {
                println!("{} Entry point: main.ts", ui::paint("[OK]", Tone::Success));
            } else {
                println!(
                    "{} No main.ts entry point was found at the project root",
                    ui::paint("[WARN]", Tone::Warning)
                );
            }
        }
        None => println!(
            "{} Not inside a BornEngine project; project checks skipped.",
            ui::paint("[INFO]", Tone::Info)
        ),
    }

    if healthy {
        println!("\n{}", ui::paint("Environment looks ready.", Tone::Success));
        Ok(0)
    } else {
        println!(
            "\n{}",
            ui::paint("Some checks need attention.", Tone::Warning)
        );
        Ok(1)
    }
}

pub fn info() -> Result<i32> {
    println!("BornEngine CLI: {}", env!("CARGO_PKG_VERSION"));
    println!(
        "Host: {} ({})",
        HostPlatform::current(),
        std::env::consts::ARCH
    );
    print_tool_version("Perry", "perry", "--version");
    print_tool_version("Rust", "rustc", "--version");
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    if let Some(root) = find_project_root(&cwd)? {
        if let Some(name) = project_name(&root)? {
            println!("Project: {name}");
        }
        if let Some(dependency) = engine_dependency(&read_package_json(&root)?)? {
            println!(
                "Engine: {} {} ({})",
                dependency.package_name,
                dependency.spec,
                dependency_source(&dependency.spec)
            );
        }
    } else {
        println!("Project: none");
    }
    Ok(0)
}

pub fn version() -> Result<i32> {
    println!("BornEngine CLI: {}", env!("CARGO_PKG_VERSION"));
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    if let Some(root) = find_project_root(&cwd)? {
        if let Some(dependency) = engine_dependency(&read_package_json(&root)?)? {
            println!(
                "BornEngine Engine: {} {}",
                dependency.package_name, dependency.spec
            );
        }
    }
    print_tool_version("Perry", "perry", "--version");
    Ok(0)
}

fn current_project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    find_project_root(&cwd)?
        .context("could not find a BornEngine project in this directory or its parents")
}

fn check_version(label: &str, executable: &str, flag: &str, healthy: &mut bool) -> Option<String> {
    match command_text(executable, &[flag]) {
        Some(version) => {
            println!("{} {label}: {version}", ui::paint("[OK]", Tone::Success));
            Some(version)
        }
        None => {
            println!(
                "{} {label} was not found or could not be started",
                ui::paint("[FAIL]", Tone::Error)
            );
            *healthy = false;
            None
        }
    }
}

fn print_tool_version(label: &str, executable: &str, flag: &str) {
    if let Some(version) = command_text(executable, &[flag]) {
        println!("{label}: {version}");
    } else {
        println!("{label}: not installed");
    }
}

fn command_text(executable: &str, args: &[&str]) -> Option<String> {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let output = captured_command(executable, &args, None, false).ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn compile_capabilities(verbose: bool) -> Result<PerryCapabilities> {
    let args = [OsString::from("compile"), OsString::from("--help")];
    let output = captured_command("perry", &args, None, verbose)?;
    if !output.status.success() {
        anyhow::bail!(
            "`perry compile --help` exited with status {}",
            output.status
        );
    }
    Ok(PerryCapabilities::from_compile_help(&format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )))
}

fn check_linux_prerequisites() -> bool {
    if !executable_in_path("pkg-config") {
        println!(
            "{} pkg-config is missing; install pkg-config and the X11/XI/ALSA development packages",
            ui::paint("[FAIL]", Tone::Error)
        );
        return false;
    }
    let mut healthy = true;
    for package in ["x11", "xi", "alsa"] {
        let args = [OsString::from("--exists"), OsString::from(package)];
        let available = captured_command("pkg-config", &args, None, false)
            .is_ok_and(|output| output.status.success());
        if available {
            println!(
                "{} Linux development library: {package}",
                ui::paint("[OK]", Tone::Success)
            );
        } else {
            println!(
                "{} Linux development library `{package}` is missing",
                ui::paint("[FAIL]", Tone::Error)
            );
            healthy = false;
        }
    }
    if !healthy {
        println!(
            "      Debian/Ubuntu: `sudo apt install pkg-config libx11-dev libxi-dev libasound2-dev`"
        );
        println!(
            "      Fedora: `sudo dnf install pkgconf-pkg-config libX11-devel libXi-devel alsa-lib-devel`"
        );
    }
    healthy
}

fn package_manager_for_project(root: &Path, config: &Config) -> Result<PackageManager> {
    let package = read_package_json(root)?;
    if let Some(value) = package.get("packageManager").and_then(Value::as_str) {
        return PackageManager::parse(value.split('@').next().unwrap_or(value));
    }
    for (file, manager) in [
        ("pnpm-lock.yaml", PackageManager::Pnpm),
        ("package-lock.json", PackageManager::Npm),
        ("yarn.lock", PackageManager::Yarn),
    ] {
        if root.join(file).is_file() {
            return Ok(manager);
        }
    }
    PackageManager::parse(&config.package_manager)
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
