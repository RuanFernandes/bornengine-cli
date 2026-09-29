use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MANIFEST_NAME: &str = "assets.manifest.json";
const MANIFEST_FORMAT: &str = "bornengine-assets-v1";
const WORLD_FORMAT: &str = "bornengine.world2d";
const WORLD_VERSION: u64 = 1;
const ASSET_ROOTS: [&str; 3] = ["assets", "public", "static"];
const SKIPPED_DIRECTORIES: [&str; 7] = [
    ".git",
    ".bornengine",
    ".perry-dev",
    "node_modules",
    "target",
    "dist",
    ".cache",
];

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AssetSummary {
    pub files: usize,
    pub bytes: u64,
    pub watch_directories: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackSummary {
    pub files: usize,
    pub bytes: u64,
}

#[derive(Deserialize, Serialize)]
struct AssetManifest {
    format: String,
    files: Vec<AssetEntry>,
}

#[derive(Deserialize, Serialize)]
struct AssetEntry {
    path: String,
    sha256: String,
    size: u64,
}

pub fn validate_project_assets(project_root: &Path) -> Result<AssetSummary> {
    let root = canonical_root(project_root)?;
    let assets = collect_project_assets(&root)?;
    let mut summary = AssetSummary::default();
    let mut watch_directories = BTreeSet::new();
    for (relative, path) in assets {
        let metadata =
            fs::metadata(&path).with_context(|| format!("could not inspect asset `{relative}`"))?;
        summary.files += 1;
        summary.bytes = summary.bytes.saturating_add(metadata.len());
        if let Some(parent) = path.parent() {
            watch_directories.insert(parent.to_path_buf());
        }
    }
    summary.watch_directories = watch_directories.into_iter().collect();
    Ok(summary)
}

pub fn pack_project(project_root: &Path, output_directory: &Path) -> Result<PackSummary> {
    let root = canonical_root(project_root)?;
    let assets = collect_project_assets(&root)?;
    let output = absolute_output_path(output_directory)?;
    for source_root in ASSET_ROOTS {
        if output.starts_with(root.join(source_root)) {
            bail!(
                "asset output {} is inside the project asset root `{source_root}`; choose a build or distribution directory",
                output.display()
            );
        }
    }
    let output_parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(output_parent)
        .with_context(|| format!("could not create output parent {}", output_parent.display()))?;
    reject_symlink_components(&output, "asset output")?;
    fs::create_dir_all(&output)
        .with_context(|| format!("could not create asset output {}", output.display()))?;
    let output = output
        .canonicalize()
        .with_context(|| format!("could not resolve asset output {}", output.display()))?;
    let old_manifest = read_existing_manifest(&output)?;
    let new_paths = assets.keys().cloned().collect::<BTreeSet<_>>();
    if let Some(manifest) = old_manifest {
        clean_stale_managed_files(&output, manifest, &new_paths)?;
    }

    let mut entries = Vec::with_capacity(assets.len());
    let mut summary = PackSummary::default();
    for (relative, source) in assets {
        let destination = output.join(Path::new(&relative));
        create_safe_parent_directories(&output, destination.parent().unwrap_or(&output))?;
        let bytes =
            fs::read(&source).with_context(|| format!("could not read asset `{relative}`"))?;
        let digest = Sha256::digest(&bytes);
        let sha256 = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        atomic_copy(&destination, &bytes)?;
        summary.files += 1;
        summary.bytes = summary.bytes.saturating_add(bytes.len() as u64);
        entries.push(AssetEntry {
            path: relative,
            sha256,
            size: bytes.len() as u64,
        });
    }
    let manifest = AssetManifest {
        format: MANIFEST_FORMAT.to_owned(),
        files: entries,
    };
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    atomic_copy(&output.join(MANIFEST_NAME), &manifest_bytes)
        .context("could not write asset manifest")?;
    Ok(summary)
}

fn read_existing_manifest(output: &Path) -> Result<Option<AssetManifest>> {
    let path = output.join(MANIFEST_NAME);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            bail!(
                "refusing to read non-regular asset manifest {}",
                path.display()
            );
        }
        Ok(_) => {
            let contents = fs::read(&path)
                .with_context(|| format!("could not read asset manifest {}", path.display()))?;
            let manifest: AssetManifest = serde_json::from_slice(&contents)
                .with_context(|| format!("invalid asset manifest {}", path.display()))?;
            if manifest.format != MANIFEST_FORMAT {
                bail!("unrecognized asset manifest format in {}", path.display());
            }
            Ok(Some(manifest))
        }
    }
}

fn clean_stale_managed_files(
    output: &Path,
    manifest: AssetManifest,
    new_paths: &BTreeSet<String>,
) -> Result<()> {
    let mut previous = BTreeSet::new();
    for entry in manifest.files {
        let path = normalize_pack_path(&entry.path)?;
        if path == MANIFEST_NAME {
            bail!("asset manifest must not list itself as a packed file");
        }
        if !previous.insert(path.clone()) {
            bail!("asset manifest contains duplicate path `{path}`");
        }
    }
    for relative in previous.difference(new_paths) {
        let path = output.join(relative);
        reject_output_parent_symlinks(output, &path)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                fs::remove_file(&path)
                    .with_context(|| format!("could not remove stale packed asset `{relative}`"))?;
            }
            Ok(_) => bail!("refusing to remove non-file packed asset `{relative}`"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect stale packed asset `{relative}`"));
            }
        }
    }
    Ok(())
}

fn normalize_pack_path(value: &str) -> Result<String> {
    if value.is_empty() || value.contains('\\') || value.starts_with('/') || value.contains(':') {
        bail!("asset manifest contains an unsafe path: `{value}`");
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("asset manifest contains an unsafe path: `{value}`");
    }
    Ok(path.to_string_lossy().replace('\\', "/"))
}

fn reject_output_parent_symlinks(output: &Path, path: &Path) -> Result<()> {
    let parent = path.parent().unwrap_or(output);
    let relative = parent
        .strip_prefix(output)
        .context("stale asset path escaped output directory")?;
    let mut current = output.to_path_buf();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            bail!("unsafe packed output path");
        };
        current.push(value);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                bail!("refusing to follow non-directory {}", current.display());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect {}", current.display()));
            }
        }
    }
    Ok(())
}

fn canonical_root(path: &Path) -> Result<PathBuf> {
    let root = path
        .canonicalize()
        .with_context(|| format!("project directory does not exist: {}", path.display()))?;
    if !root.is_dir() {
        bail!("project root is not a directory: {}", root.display());
    }
    Ok(root)
}

fn collect_project_assets(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut files = BTreeMap::new();
    let mut spelling_by_normalized = BTreeMap::new();
    for directory in ASSET_ROOTS {
        let path = root.join(directory);
        if fs::symlink_metadata(&path).is_ok() {
            collect_asset_tree(
                root,
                &path,
                &mut files,
                &mut spelling_by_normalized,
                &mut BTreeSet::new(),
            )?;
        }
    }
    collect_world_documents(root, root, &mut files, &mut spelling_by_normalized)?;
    Ok(files)
}

fn collect_asset_tree(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, PathBuf>,
    spelling_by_normalized: &mut BTreeMap<String, String>,
    ancestors: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("could not inspect {}", directory.display()))?;
    if metadata.file_type().is_symlink() {
        let target = directory
            .canonicalize()
            .with_context(|| format!("could not resolve symbolic link {}", directory.display()))?;
        ensure_inside_root(root, &target, "asset directory")?;
        if target.is_dir() {
            if !ancestors.insert(target.clone()) {
                return Ok(());
            }
            for entry in fs::read_dir(directory)
                .with_context(|| format!("could not read {}", directory.display()))?
            {
                collect_asset_tree(
                    root,
                    &entry?.path(),
                    files,
                    spelling_by_normalized,
                    ancestors,
                )?;
            }
            ancestors.remove(&target);
            return Ok(());
        }
        add_asset(root, directory, files, spelling_by_normalized)?;
        return Ok(());
    }
    if metadata.is_file() {
        add_asset(root, directory, files, spelling_by_normalized)?;
        return Ok(());
    }
    if !metadata.is_dir() {
        bail!("unsupported asset file type: {}", directory.display());
    }
    let canonical = directory
        .canonicalize()
        .with_context(|| format!("could not resolve {}", directory.display()))?;
    ensure_inside_root(root, &canonical, "asset directory")?;
    if !ancestors.insert(canonical.clone()) {
        return Ok(());
    }
    for entry in fs::read_dir(directory)
        .with_context(|| format!("could not read {}", directory.display()))?
    {
        let path = entry?.path();
        collect_asset_tree(root, &path, files, spelling_by_normalized, ancestors)?;
    }
    ancestors.remove(&canonical);
    Ok(())
}

fn collect_world_documents(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, PathBuf>,
    spelling_by_normalized: &mut BTreeMap<String, String>,
) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("could not inspect {}", directory.display()))?;
    if metadata.file_type().is_symlink() {
        let target = directory
            .canonicalize()
            .with_context(|| format!("could not resolve symbolic link {}", directory.display()))?;
        ensure_inside_root(root, &target, "project file")?;
        if target.is_dir() {
            return Ok(());
        }
        if is_world_document(directory) {
            collect_world_document(root, directory, files, spelling_by_normalized)?;
        }
        return Ok(());
    }
    if metadata.is_file() {
        if is_world_document(directory) {
            collect_world_document(root, directory, files, spelling_by_normalized)?;
        }
        return Ok(());
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)
        .with_context(|| format!("could not read {}", directory.display()))?
    {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if directory == root && SKIPPED_DIRECTORIES.contains(&name) {
            continue;
        }
        collect_world_documents(root, &path, files, spelling_by_normalized)?;
    }
    Ok(())
}

fn collect_world_document(
    root: &Path,
    path: &Path,
    files: &mut BTreeMap<String, PathBuf>,
    spelling_by_normalized: &mut BTreeMap<String, String>,
) -> Result<()> {
    let bytes = fs::read(path)
        .with_context(|| format!("could not read world document {}", path.display()))?;
    let world: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("{}: invalid BornEngine world JSON", path.display()))?;
    if world.get("format").and_then(Value::as_str) != Some(WORLD_FORMAT) {
        return Ok(());
    }
    let version = world.get("version").and_then(Value::as_u64);
    if version != Some(WORLD_VERSION) {
        bail!(
            "{}: unsupported world format version {:?}",
            path.display(),
            version
        );
    }
    let listed_assets = world
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("{}: `assets` must be an array", path.display()))?;
    let mut seen = BTreeSet::new();
    for value in listed_assets {
        let raw = value
            .as_str()
            .ok_or_else(|| anyhow!("{}: every asset path must be a string", path.display()))?;
        let relative = normalize_reference(raw, path)?;
        if !seen.insert(relative.clone()) {
            bail!(
                "{}: duplicate normalized asset path `{relative}`",
                path.display()
            );
        }
        let resolved = resolve_project_reference(root, &relative, path)?;
        add_normalized_asset(root, &relative, resolved, files, spelling_by_normalized)?;
    }
    Ok(())
}

fn add_asset(
    root: &Path,
    path: &Path,
    files: &mut BTreeMap<String, PathBuf>,
    spelling_by_normalized: &mut BTreeMap<String, String>,
) -> Result<()> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("could not resolve project asset {}", path.display()))?;
    ensure_inside_root(root, &canonical, "asset")?;
    if !canonical.is_file() {
        return Ok(());
    }
    let relative = path
        .strip_prefix(root)
        .context("asset escaped project root")?
        .to_string_lossy()
        .replace('\\', "/");
    add_normalized_asset(root, &relative, canonical, files, spelling_by_normalized)
}

fn add_normalized_asset(
    _root: &Path,
    normalized: &str,
    path: PathBuf,
    files: &mut BTreeMap<String, PathBuf>,
    spelling_by_normalized: &mut BTreeMap<String, String>,
) -> Result<()> {
    if let Some(previous) = spelling_by_normalized.get(normalized) {
        if previous != normalized {
            bail!("asset paths `{previous}` and `{normalized}` normalize to the same project path");
        }
        files.entry(normalized.to_owned()).or_insert(path);
        return Ok(());
    }
    spelling_by_normalized.insert(normalized.to_owned(), normalized.to_owned());
    files.insert(normalized.to_owned(), path);
    Ok(())
}

fn normalize_reference(raw: &str, world_file: &Path) -> Result<String> {
    if raw.is_empty() || raw.contains('\\') || raw.starts_with('/') || raw.contains(':') {
        bail!("{}: unsafe asset path `{raw}`", world_file.display());
    }
    let path = Path::new(raw);
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("{}: unsafe asset path `{raw}`", world_file.display());
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("{}: unsafe asset path `{raw}`", world_file.display());
    }
    Ok(normalized)
}

fn resolve_project_reference(root: &Path, relative: &str, world_file: &Path) -> Result<PathBuf> {
    let path = root.join(relative);
    match path.canonicalize() {
        Ok(canonical) => {
            ensure_inside_root(
                root,
                &canonical,
                &format!("asset `{relative}` in {}", world_file.display()),
            )?;
            if !canonical.is_file() {
                bail!(
                    "{}: referenced asset `{relative}` is not a file",
                    world_file.display()
                );
            }
            Ok(canonical)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let suggestion = case_mismatch_suggestion(root, relative);
            if let Some(suggestion) = suggestion {
                bail!(
                    "{}: referenced asset `{relative}` does not match disk case; did you mean `{suggestion}`?",
                    world_file.display()
                );
            }
            bail!(
                "{}: referenced asset `{relative}` does not exist",
                world_file.display()
            );
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "{}: could not resolve asset `{relative}`",
                world_file.display()
            )
        }),
    }
}

fn case_mismatch_suggestion(root: &Path, relative: &str) -> Option<String> {
    let mut current = root.to_path_buf();
    let mut actual = Vec::new();
    for part in relative.split('/') {
        let entries = fs::read_dir(&current).ok()?;
        let matching = entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .find(|name| name.to_string_lossy().eq_ignore_ascii_case(part))?;
        actual.push(matching.to_string_lossy().into_owned());
        current.push(matching);
    }
    let result = actual.join("/");
    (result != relative).then_some(result)
}

fn ensure_inside_root(root: &Path, path: &Path, label: &str) -> Result<()> {
    if !path.starts_with(root) {
        bail!(
            "{label} resolves outside the project root: {}",
            path.display()
        );
    }
    Ok(())
}

fn is_world_document(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".world2d.json"))
}

fn absolute_output_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn reject_symlink_components(path: &Path, label: &str) -> Result<()> {
    let absolute = absolute_output_path(path)?;
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "refusing to use symbolic link in {label} path: {}",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect {}", current.display()));
            }
        }
    }
    Ok(())
}

fn create_safe_parent_directories(root: &Path, parent: &Path) -> Result<()> {
    if !parent.starts_with(root) {
        bail!("packed asset path escaped output directory");
    }
    let relative = parent.strip_prefix(root).unwrap_or(Path::new(""));
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            bail!("unsafe packed output path")
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                bail!(
                    "refusing to write packed asset through non-directory {}",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .with_context(|| format!("could not create {}", current.display()))?;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect {}", current.display()));
            }
        }
    }
    Ok(())
}

fn atomic_copy(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("output path has no parent: {}", path.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("output path has no file name: {}", path.display()))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    fs::write(&temporary, bytes)
        .with_context(|| format!("could not write {}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("could not replace {}", path.display()));
    }
    Ok(())
}
