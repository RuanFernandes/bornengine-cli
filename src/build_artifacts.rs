use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MANIFEST_NAME: &str = "manifest.json";
const MANIFEST_FORMAT: &str = "bornengine-build-manifest-v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildArtifact {
    pub directory: PathBuf,
    pub output: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BuildManifest {
    format: String,
    files: Vec<String>,
}

impl Default for BuildManifest {
    fn default() -> Self {
        Self {
            format: MANIFEST_FORMAT.to_owned(),
            files: Vec::new(),
        }
    }
}

pub fn begin_build(
    project_root: &Path,
    target: &str,
    name: &str,
    extension: Option<&str>,
) -> Result<BuildArtifact> {
    let target = validate_segment(target, "target")?;
    let name = validate_segment(name, "output name")?;
    let builds_root = ensure_builds_root(project_root)?;
    let target_dir = ensure_child_directory(&builds_root, &target)?;
    let name_dir = ensure_child_directory(&target_dir, &name)?;
    create_build_artifact(&name_dir, name, extension)
}

pub fn begin_dev_build(
    project_root: &Path,
    name: &str,
    extension: Option<&str>,
) -> Result<BuildArtifact> {
    let name = validate_segment(name, "output name")?;
    let dev_root = ensure_child_directory(project_root, ".perry-dev")?;
    let name_dir = ensure_child_directory(&dev_root, &name)?;
    create_build_artifact(&name_dir, name, extension)
}

fn create_build_artifact(
    name_dir: &Path,
    mut output_name: String,
    extension: Option<&str>,
) -> Result<BuildArtifact> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut attempt = 0_u32;
    let build_dir = loop {
        let candidate = name_dir.join(format!("{timestamp}-{}-{attempt}", std::process::id()));
        match fs::create_dir(&candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                attempt = attempt.saturating_add(1);
                if attempt == u32::MAX {
                    bail!(
                        "could not allocate a unique build directory under {}",
                        name_dir.display()
                    );
                }
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not create {}", candidate.display()));
            }
        }
    };
    if let Some(extension) = extension {
        let extension = extension.trim_start_matches('.');
        if !output_name
            .to_ascii_lowercase()
            .ends_with(&format!(".{extension}").to_ascii_lowercase())
        {
            output_name.push('.');
            output_name.push_str(extension);
        }
    }
    Ok(BuildArtifact {
        output: build_dir.join(output_name),
        directory: build_dir,
    })
}

pub fn record_build_files(project_root: &Path, build_directory: &Path) -> Result<usize> {
    let builds_root = ensure_builds_root(project_root)?;
    let project_root = project_root
        .canonicalize()
        .with_context(|| format!("could not resolve {}", project_root.display()))?;
    let build_directory = build_directory
        .canonicalize()
        .with_context(|| format!("could not resolve {}", build_directory.display()))?;
    let relative_directory = build_directory
        .strip_prefix(&project_root)
        .context("build output directory is outside the project")?;
    validate_managed_project_path(relative_directory)?;

    let mut files = Vec::new();
    collect_files(&build_directory, &project_root, &mut files)?;
    files.sort();
    let additions = files.len();
    let manifest_path = builds_root.join(MANIFEST_NAME);
    ensure_manifest_is_regular_or_missing(&manifest_path)?;
    let mut manifest = read_manifest_or_default(&manifest_path)?;
    validate_manifest_entries(&project_root, &manifest.files)?;
    let mut all_files = manifest.files.into_iter().collect::<BTreeSet<_>>();
    all_files.extend(files);
    manifest.files = all_files.into_iter().collect();
    write_manifest(&manifest_path, &manifest)?;
    Ok(additions)
}

pub fn clean_build_artifacts(project_root: &Path) -> Result<usize> {
    let Some(engine_dir) = existing_plain_directory(&project_root.join(".bornengine"))? else {
        return Ok(0);
    };
    let Some(builds_root) = existing_plain_directory(&engine_dir.join("builds"))? else {
        return Ok(0);
    };
    let manifest_path = builds_root.join(MANIFEST_NAME);
    ensure_manifest_is_regular_or_missing(&manifest_path)?;
    if !manifest_path.is_file() {
        return Ok(0);
    }
    let mut manifest = read_manifest(&manifest_path)?;
    validate_manifest_entries(project_root, &manifest.files)?;
    let files = manifest.files.iter().collect::<BTreeSet<_>>();
    let mut removed = 0;
    let mut parent_directories = BTreeSet::new();
    for relative in files {
        let path = project_root.join(relative);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                fs::remove_file(&path)
                    .with_context(|| format!("could not remove {}", path.display()))?;
                removed += 1;
                if let Some(parent) = path.parent() {
                    let protected_root = managed_area_root(project_root, Path::new(relative))?;
                    if parent != protected_root.as_path() {
                        parent_directories.insert((protected_root, parent.to_path_buf()));
                    }
                }
            }
            Ok(_) => bail!(
                "refusing to remove non-file build artifact {}",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("could not inspect {}", path.display()));
            }
        }
    }
    for (protected_root, directory) in parent_directories.into_iter().rev() {
        let mut current = Some(directory.as_path());
        while let Some(path) = current {
            if path == protected_root {
                break;
            }
            if fs::remove_dir(path).is_err() {
                break;
            }
            current = path.parent();
        }
    }
    manifest.files.clear();
    write_manifest(&manifest_path, &manifest)?;
    Ok(removed)
}

fn ensure_builds_root(project_root: &Path) -> Result<PathBuf> {
    if !project_root.is_dir() {
        bail!(
            "project directory does not exist: {}",
            project_root.display()
        );
    }
    let engine_dir = ensure_child_directory(project_root, ".bornengine")?;
    ensure_child_directory(&engine_dir, "builds")
}

fn ensure_child_directory(parent: &Path, child: &str) -> Result<PathBuf> {
    let path = parent.join(child);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "refusing to use symbolic link as a build directory: {}",
                path.display()
            );
        }
        Ok(metadata) if metadata.is_dir() => Ok(path),
        Ok(_) => bail!(
            "build path exists and is not a directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&path)
                .with_context(|| format!("could not create build directory {}", path.display()))?;
            Ok(path)
        }
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
    }
}

fn existing_plain_directory(path: &Path) -> Result<Option<PathBuf>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "refusing to inspect symbolic link in CLI build area: {}",
                path.display()
            );
        }
        Ok(metadata) if metadata.is_dir() => Ok(Some(path.to_path_buf())),
        Ok(_) => bail!("CLI build path is not a directory: {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
    }
}

fn validate_segment(value: &str, label: &str) -> Result<String> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        bail!("{label} must be a simple file or directory name");
    }
    Ok(value.to_owned())
}

fn collect_files(directory: &Path, project_root: &Path, files: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("could not inspect {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files(&path, project_root, files)?;
        } else {
            let relative = path
                .strip_prefix(project_root)
                .context("build output escaped the project directory")?;
            validate_relative_path(relative)?;
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<BuildManifest> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("could not read build manifest {}", path.display()))?;
    let manifest: BuildManifest = serde_json::from_str(&contents)
        .with_context(|| format!("invalid build manifest {}", path.display()))?;
    if manifest.format != MANIFEST_FORMAT {
        bail!("unrecognized build manifest format in {}", path.display());
    }
    Ok(manifest)
}

fn read_manifest_or_default(path: &Path) -> Result<BuildManifest> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BuildManifest::default()),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
        Ok(_) => read_manifest(path),
    }
}

fn write_manifest(path: &Path, manifest: &BuildManifest) -> Result<()> {
    let mut contents = serde_json::to_string_pretty(manifest)?;
    contents.push('\n');
    fs::write(path, contents)
        .with_context(|| format!("could not write build manifest {}", path.display()))
}

fn ensure_manifest_is_regular_or_missing(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            bail!(
                "refusing to use non-regular build manifest {}",
                path.display()
            );
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
    }
}

fn validate_manifest_entries(project_root: &Path, files: &[String]) -> Result<()> {
    for file in files {
        let relative = Path::new(file);
        validate_relative_path(relative)?;
        validate_managed_project_path(relative)?;
        let mut current = project_root.to_path_buf();
        let components = relative.components().collect::<Vec<_>>();
        for (index, component) in components.iter().enumerate() {
            current.push(component.as_os_str());
            if index + 1 == components.len() {
                continue;
            }
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!(
                        "refusing to follow symbolic link in build manifest path {}",
                        current.display()
                    );
                }
                Ok(metadata) if !metadata.is_dir() => break,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("could not inspect {}", current.display()));
                }
            }
        }
    }
    Ok(())
}

fn validate_managed_project_path(path: &Path) -> Result<()> {
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let builds_area =
        components.len() >= 3 && components[0] == ".bornengine" && components[1] == "builds";
    let dev_area = components.len() >= 2 && components[0] == ".perry-dev";
    if !builds_area && !dev_area {
        bail!(
            "build manifest path is outside CLI-managed build areas: {}",
            path.display()
        );
    }
    Ok(())
}

fn managed_area_root(project_root: &Path, relative: &Path) -> Result<PathBuf> {
    let first = relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str());
    match first {
        Some(".perry-dev") => Ok(project_root.join(".perry-dev")),
        Some(".bornengine") => Ok(project_root.join(".bornengine").join("builds")),
        _ => bail!("build manifest path is outside CLI-managed build areas"),
    }
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("build manifest contains an unsafe path: {}", path.display());
    }
    Ok(())
}
