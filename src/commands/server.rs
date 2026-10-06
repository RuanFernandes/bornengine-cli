use crate::commands::project::{ensure_manager, select_package_manager};
use crate::config::Config;
use crate::package_manager::PackageManager;
use crate::process::inherited_command;
use crate::project::find_project_root;
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

const SERVER_MARKER_FILE: &str = "bornengine.server.json";

pub fn create(
    requested_path: Option<&Path>,
    package_manager: Option<PackageManager>,
    verbose: bool,
) -> Result<i32> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let cwd = cwd
        .canonicalize()
        .context("could not resolve current directory")?;
    let project_root = find_project_root(&cwd)?.context(
        "No BornEngine project found. Run `bornengine create server` from inside a BornEngine game project.",
    )?;
    let project_root = project_root
        .canonicalize()
        .context("could not resolve BornEngine project root")?;

    let target = resolve_server_target(&project_root, requested_path)?;
    ensure_target_available(&target)?;

    let config = Config::load_default()?;
    let manager = select_package_manager(package_manager, &config)?;
    ensure_manager(manager)?;

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "could not create server parent directory {}",
                parent.display()
            )
        })?;
    }

    let (generator, mut args) = colyseus_generator(manager);
    args.push(target.as_os_str().to_owned());
    let status = inherited_command(generator, &args, Some(&project_root), verbose)?;
    if status != 0 {
        return Ok(status);
    }

    write_server_marker(&project_root, &target)?;
    Ok(0)
}

fn colyseus_generator(manager: PackageManager) -> (&'static str, Vec<OsString>) {
    let args = match manager {
        PackageManager::Npm | PackageManager::Pnpm => ["create", "colyseus-app@latest"],
        PackageManager::Yarn => ["create", "colyseus-app"],
    }
    .into_iter()
    .map(OsString::from)
    .collect();
    (manager.executable(), args)
}

fn resolve_server_target(project_root: &Path, requested_path: Option<&Path>) -> Result<PathBuf> {
    let requested_path = requested_path.unwrap_or_else(|| Path::new("server"));
    let candidate = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        project_root.join(requested_path)
    };
    let candidate = normalize_absolute(&candidate)?;

    if candidate == project_root || !candidate.starts_with(project_root) {
        bail!(
            "server path must resolve to a strict child inside the BornEngine project: {}",
            candidate.display()
        );
    }

    let relative = candidate
        .strip_prefix(project_root)
        .context("server path could not be made relative to the BornEngine project")?;
    let components = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut current = project_root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "server path must not traverse symbolic links: {}",
                    current.display()
                );
            }
            Ok(metadata) if index + 1 < components.len() && !metadata.is_dir() => {
                bail!(
                    "server path parent is not a directory: {}",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("could not inspect server path {}", current.display())
                });
            }
        }
    }

    Ok(candidate)
}

fn normalize_absolute(path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    bail!("server path must be an absolute path inside the BornEngine project");
                }
            }
            Component::Normal(name) => normalized.push(name),
        }
    }
    if !normalized.is_absolute() {
        bail!("server path could not be resolved to an absolute path");
    }
    Ok(normalized)
}

fn ensure_target_available(target: &Path) -> Result<()> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "refusing symbolic link as server destination: {}",
                target.display()
            );
        }
        Ok(metadata) if !metadata.is_dir() => {
            bail!(
                "server destination is not a directory: {}",
                target.display()
            );
        }
        Ok(_) => {
            let mut entries = fs::read_dir(target).with_context(|| {
                format!("could not inspect server destination {}", target.display())
            })?;
            if entries.next().is_some() {
                bail!("server destination is not empty: {}", target.display());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!("could not inspect server destination {}", target.display())
            });
        }
    }
    Ok(())
}

fn write_server_marker(project_root: &Path, server_root: &Path) -> Result<()> {
    let relative_server = server_root
        .strip_prefix(project_root)
        .context("server destination is not inside the BornEngine project")?;
    let depth = relative_server
        .components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count();
    let mut relative_client_root = PathBuf::new();
    for _ in 0..depth {
        relative_client_root.push("..");
    }
    let relative_client_root = relative_client_root.to_string_lossy().replace('\\', "/");
    let marker = json!({
        "format": "bornengine.server",
        "version": 1,
        "provider": "colyseus",
        "clientProjectRoot": relative_client_root,
    });
    let mut contents = serde_json::to_vec_pretty(&marker)
        .context("could not serialize the BornEngine server marker")?;
    contents.push(b'\n');

    let marker_path = server_root.join(SERVER_MARKER_FILE);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker_path)
        .with_context(|| {
            format!(
                "the Colyseus server was generated at {}, but its BornEngine marker could not be created at {}; preserve the server and create the marker manually",
                server_root.display(),
                marker_path.display()
            )
        })?;
    if let Err(error) = file.write_all(&contents) {
        let _ = fs::remove_file(&marker_path);
        return Err(error).with_context(|| {
            format!(
                "the Colyseus server was generated at {}, but writing {} failed; preserve the server and create the marker manually",
                server_root.display(),
                marker_path.display()
            )
        });
    }
    Ok(())
}
