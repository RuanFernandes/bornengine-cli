use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MANIFEST_NAME: &str = "assets.manifest.json";
const AUDIT_MANIFEST_NAME: &str = "bornengine.assets.json";
const AUDIT_REPORT_FORMAT: &str = "bornengine.asset_validation";
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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AssetSummary {
    pub files: usize,
    pub bytes: u64,
    #[serde(skip)]
    pub watch_directories: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "kebab-case")]
pub enum OrphanPolicy {
    Ignore,
    #[default]
    Warn,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetSeverity {
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetDiagnosticCode {
    OrphanAsset,
    InvalidMediaHeader,
    ExtensionMismatch,
    FileSizeLimit,
    TotalSizeLimit,
    ImageDimensionLimit,
    TotalImagePixelsLimit,
    DeclaredDynamicPathMissing,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AssetValidationOptions {
    pub orphan_policy: Option<OrphanPolicy>,
    pub max_file_bytes: Option<u64>,
    pub max_total_bytes: Option<u64>,
    pub max_image_dimension: Option<u32>,
    pub max_total_image_pixels: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetValidationSettings {
    pub dynamic_paths: Vec<String>,
    pub ignored_paths: Vec<String>,
    pub orphan_policy: OrphanPolicy,
    pub max_file_bytes: Option<u64>,
    pub max_total_bytes: Option<u64>,
    pub max_image_dimension: Option<u32>,
    pub max_total_image_pixels: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssetDiagnostic {
    pub code: AssetDiagnosticCode,
    pub severity: AssetSeverity,
    pub path: String,
    pub message: String,
    pub measured: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssetValidationReport {
    pub format: &'static str,
    pub version: u32,
    pub summary: AssetSummary,
    pub diagnostics: Vec<AssetDiagnostic>,
}

impl AssetValidationReport {
    pub fn new(summary: AssetSummary, mut diagnostics: Vec<AssetDiagnostic>) -> Result<Self> {
        for diagnostic in &diagnostics {
            if !diagnostic.path.is_empty() {
                validate_audit_path(&diagnostic.path, false)
                    .with_context(|| format!("unsafe diagnostic path `{}`", diagnostic.path))?;
            }
        }
        diagnostics.sort_by(|a, b| {
            (
                &a.path, &a.code, a.severity, &a.message, a.measured, a.limit,
            )
                .cmp(&(
                    &b.path, &b.code, b.severity, &b.message, b.measured, b.limit,
                ))
        });
        Ok(Self {
            format: AUDIT_REPORT_FORMAT,
            version: 1,
            summary,
            diagnostics,
        })
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == AssetSeverity::Error)
    }
}

pub fn load_asset_validation_settings(
    project_root: &Path,
    options: &AssetValidationOptions,
) -> Result<AssetValidationSettings> {
    let root = canonical_root(project_root)?;
    let manifest_path = root.join(AUDIT_MANIFEST_NAME);
    let document = match fs::symlink_metadata(&manifest_path) {
        Ok(_) => {
            let canonical = manifest_path
                .canonicalize()
                .with_context(|| format!("could not resolve {AUDIT_MANIFEST_NAME}"))?;
            ensure_inside_root(&root, &canonical, AUDIT_MANIFEST_NAME)?;
            let bytes = fs::read(&manifest_path)
                .with_context(|| format!("could not read {AUDIT_MANIFEST_NAME}"))?;
            Some(
                serde_json::from_slice::<Value>(&bytes)
                    .with_context(|| format!("invalid {AUDIT_MANIFEST_NAME}"))?,
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| format!("could not inspect {AUDIT_MANIFEST_NAME}"));
        }
    };
    let mut settings = AssetValidationSettings {
        dynamic_paths: Vec::new(),
        ignored_paths: Vec::new(),
        orphan_policy: OrphanPolicy::Warn,
        max_file_bytes: None,
        max_total_bytes: None,
        max_image_dimension: None,
        max_total_image_pixels: None,
    };
    if let Some(document) = document {
        let object = document
            .as_object()
            .ok_or_else(|| anyhow!("{AUDIT_MANIFEST_NAME}: expected a JSON object"))?;
        if object.get("version").and_then(Value::as_u64) != Some(1) {
            bail!("{AUDIT_MANIFEST_NAME}: unsupported version; expected version 1");
        }
        for key in object.keys() {
            if ![
                "version",
                "dynamic_paths",
                "ignored_paths",
                "orphan_severity",
                "max_file_bytes",
                "max_total_bytes",
                "max_image_dimension",
                "max_total_image_pixels",
            ]
            .contains(&key.as_str())
            {
                bail!("{AUDIT_MANIFEST_NAME}: unsupported field `{key}`");
            }
        }
        settings.dynamic_paths = manifest_paths_field(object, "dynamic_paths", false)?;
        settings.ignored_paths = manifest_paths_field(object, "ignored_paths", true)?;
        if let Some(value) = object.get("orphan_severity") {
            settings.orphan_policy = match value.as_str() {
                Some("ignore") => OrphanPolicy::Ignore,
                Some("warning") | Some("warn") => OrphanPolicy::Warn,
                Some("error") => OrphanPolicy::Error,
                _ => bail!("{AUDIT_MANIFEST_NAME}: invalid orphan_severity"),
            };
        }
        settings.max_file_bytes = positive_limit(object, "max_file_bytes")?;
        settings.max_total_bytes = positive_limit(object, "max_total_bytes")?;
        settings.max_total_image_pixels = positive_limit(object, "max_total_image_pixels")?;
        settings.max_image_dimension = positive_limit(object, "max_image_dimension")?
            .map(|value| {
                u32::try_from(value).map_err(|_| {
                    anyhow!("{AUDIT_MANIFEST_NAME}: max_image_dimension exceeds u32 range")
                })
            })
            .transpose()?;
    }
    if let Some(value) = options.orphan_policy {
        settings.orphan_policy = value;
    }
    if let Some(value) = options.max_file_bytes {
        settings.max_file_bytes = Some(checked_option_limit(value, "max_file_bytes")?);
    }
    if let Some(value) = options.max_total_bytes {
        settings.max_total_bytes = Some(checked_option_limit(value, "max_total_bytes")?);
    }
    if let Some(value) = options.max_image_dimension {
        settings.max_image_dimension = Some(u32::try_from(checked_option_limit(
            u64::from(value),
            "max_image_dimension",
        )?)?);
    }
    if let Some(value) = options.max_total_image_pixels {
        settings.max_total_image_pixels =
            Some(checked_option_limit(value, "max_total_image_pixels")?);
    }
    Ok(settings)
}

fn manifest_paths_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
    globs: bool,
) -> Result<Vec<String>> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| anyhow!("{AUDIT_MANIFEST_NAME}: `{key}` must be an array"))?;
    let mut paths = BTreeSet::new();
    for value in values {
        let path = value
            .as_str()
            .ok_or_else(|| anyhow!("{AUDIT_MANIFEST_NAME}: `{key}` entries must be strings"))?;
        validate_audit_path(path, globs)?;
        if !paths.insert(path.to_owned()) {
            bail!("{AUDIT_MANIFEST_NAME}: duplicate `{key}` entry `{path}`");
        }
    }
    Ok(paths.into_iter().collect())
}

fn validate_audit_path(path: &str, globs: bool) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        bail!("{AUDIT_MANIFEST_NAME}: unsafe project-relative path `{path}`");
    }
    for component in path.split('/') {
        if component.contains(['?', '[', ']', '{', '}'])
            || (!globs && component.contains('*'))
            || (globs && component.contains("**") && component != "**")
        {
            bail!("{AUDIT_MANIFEST_NAME}: unsupported glob syntax in `{path}`");
        }
    }
    Ok(())
}

fn positive_limit(object: &serde_json::Map<String, Value>, key: &str) -> Result<Option<u64>> {
    object
        .get(key)
        .map(|value| {
            let number = value.as_u64().ok_or_else(|| {
                anyhow!("{AUDIT_MANIFEST_NAME}: `{key}` must be a positive integer")
            })?;
            checked_option_limit(number, key)
        })
        .transpose()
}

fn checked_option_limit(value: u64, key: &str) -> Result<u64> {
    if value == 0 {
        bail!("{key} must be greater than zero");
    }
    Ok(value)
}

pub fn validate_project_assets_with_options(
    project_root: &Path,
    options: &AssetValidationOptions,
) -> Result<AssetValidationReport> {
    let _settings = load_asset_validation_settings(project_root, options)?;
    let summary = validate_project_assets(project_root)?;
    AssetValidationReport::new(summary, Vec::new())
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
    pack_project_with_rename(project_root, output_directory, |source, destination| {
        fs::rename(source, destination)
    })
}

fn pack_project_with_rename<F>(
    project_root: &Path,
    output_directory: &Path,
    mut rename: F,
) -> Result<PackSummary>
where
    F: FnMut(&Path, &Path) -> io::Result<()>,
{
    let root = canonical_root(project_root)?;
    let assets = collect_project_assets(&root)?;
    let output_input = absolute_output_path(output_directory)?;
    reject_symlink_components(&output_input, "asset output")?;
    let mut output = canonicalize_existing_prefix(&output_input)?;
    reject_asset_root_output(&root, &output)?;
    if assets.contains_key(MANIFEST_NAME) {
        bail!("project asset path `{MANIFEST_NAME}` is reserved for the pack manifest");
    }
    let output_parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&output_parent)
        .with_context(|| format!("could not create output parent {}", output_parent.display()))?;
    reject_symlink_components(&output, "asset output")?;
    output = canonicalize_existing_prefix(&output)?;
    reject_asset_root_output(&root, &output)?;
    let output_exists = validate_pack_output_directory(&output)?;
    let old_manifest = if output_exists {
        read_existing_manifest(&output)?
    } else {
        None
    };
    let old_paths = manifest_paths(old_manifest.as_ref())?;
    let new_paths = assets.keys().cloned().collect::<BTreeSet<_>>();
    preflight_pack_output(&output, &old_paths, &new_paths)?;

    let stage = PackStage::create(&output_parent)?;
    let staged_files = stage.0.join("next");
    fs::create_dir(&staged_files).with_context(|| {
        format!(
            "could not create staging directory {}",
            staged_files.display()
        )
    })?;

    let mut entries = Vec::with_capacity(assets.len());
    let mut summary = PackSummary::default();
    for (relative, source) in assets {
        let bytes =
            fs::read(&source).with_context(|| format!("could not read asset `{relative}`"))?;
        let digest = Sha256::digest(&bytes);
        let sha256 = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let destination = staged_files.join(Path::new(&relative));
        let parent = destination
            .parent()
            .context("staged asset path has no parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create staging directory {}", parent.display()))?;
        fs::write(&destination, &bytes)
            .with_context(|| format!("could not stage asset `{relative}`"))?;
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
    fs::write(staged_files.join(MANIFEST_NAME), &manifest_bytes)
        .context("could not stage asset manifest")?;

    publish_staged_pack(
        &output,
        &stage.0,
        &staged_files,
        &old_paths,
        &new_paths,
        output_exists,
        &mut rename,
    )?;
    Ok(summary)
}

struct PackStage(PathBuf);

impl PackStage {
    fn create(parent: &Path) -> Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for attempt in 0_u32..u32::MAX {
            let candidate = parent.join(format!(
                ".bornengine-assets-{}-{timestamp}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => return Ok(Self(candidate)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "could not create pack staging directory {}",
                            candidate.display()
                        )
                    });
                }
            }
        }
        bail!("could not allocate a unique asset pack staging directory")
    }
}

impl Drop for PackStage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn validate_pack_output_directory(output: &Path) -> Result<bool> {
    match fs::symlink_metadata(output) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "refusing to use symbolic link as asset output: {}",
                output.display()
            );
        }
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => bail!(
            "asset output exists and is not a directory: {}",
            output.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", output.display())),
    }
}

fn manifest_paths(manifest: Option<&AssetManifest>) -> Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    if let Some(manifest) = manifest {
        for entry in &manifest.files {
            let path = normalize_pack_path(&entry.path)?;
            if path == MANIFEST_NAME {
                bail!("asset manifest must not list itself as a packed file");
            }
            if !paths.insert(path.clone()) {
                bail!("asset manifest contains duplicate path `{path}`");
            }
        }
    }
    Ok(paths)
}

fn preflight_pack_output(
    output: &Path,
    old_paths: &BTreeSet<String>,
    new_paths: &BTreeSet<String>,
) -> Result<()> {
    for relative in old_paths.union(new_paths) {
        let path = output.join(relative);
        reject_output_parent_symlinks(output, &path)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("refusing to replace symbolic link packed asset `{relative}`");
            }
            Ok(metadata) if !metadata.is_file() => {
                bail!("packed asset path `{relative}` is not a regular file");
            }
            Ok(_) if new_paths.contains(relative) && !old_paths.contains(relative) => {
                bail!("refusing to replace untracked output file `{relative}`");
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect packed asset `{relative}`"));
            }
        }
    }
    Ok(())
}

fn publish_staged_pack<F>(
    output: &Path,
    stage_root: &Path,
    staged_files: &Path,
    old_paths: &BTreeSet<String>,
    new_paths: &BTreeSet<String>,
    output_exists: bool,
    rename: &mut F,
) -> Result<()>
where
    F: FnMut(&Path, &Path) -> io::Result<()>,
{
    let mut output_created = false;
    if !output_exists {
        fs::create_dir(output)
            .with_context(|| format!("could not create asset output {}", output.display()))?;
        output_created = true;
    }

    let mut backup_paths = old_paths.clone();
    backup_paths.insert(MANIFEST_NAME.to_owned());
    let mut moved_to_backup = Vec::<(PathBuf, PathBuf)>::new();
    let mut published = Vec::<PathBuf>::new();
    let mut created_directories = Vec::<PathBuf>::new();

    let publish_result = (|| -> Result<()> {
        for relative in &backup_paths {
            let destination = output.join(relative);
            match fs::symlink_metadata(&destination) {
                Ok(_) => {
                    let backup = stage_root.join("previous").join(relative);
                    let backup_parent = backup
                        .parent()
                        .context("asset backup path has no parent directory")?;
                    fs::create_dir_all(backup_parent).with_context(|| {
                        format!(
                            "could not create backup directory {}",
                            backup_parent.display()
                        )
                    })?;
                    rename(&destination, &backup).with_context(|| {
                        format!("could not back up existing asset {}", destination.display())
                    })?;
                    moved_to_backup.push((backup, destination));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("could not inspect existing asset {}", destination.display())
                    });
                }
            }
        }

        for relative in new_paths {
            let source = staged_files.join(relative);
            let destination = output.join(relative);
            create_safe_parent_directories(
                output,
                destination.parent().unwrap_or(output),
                &mut created_directories,
            )?;
            rename(&source, &destination)
                .with_context(|| format!("could not publish packed asset `{relative}`"))?;
            published.push(destination);
        }

        let source_manifest = staged_files.join(MANIFEST_NAME);
        let destination_manifest = output.join(MANIFEST_NAME);
        rename(&source_manifest, &destination_manifest)
            .context("could not publish asset manifest")?;
        published.push(destination_manifest);
        Ok(())
    })();

    if let Err(error) = publish_result {
        let rollback_errors = rollback_pack(
            output,
            &moved_to_backup,
            &published,
            &created_directories,
            output_created,
            rename,
        );
        if rollback_errors.is_empty() {
            return Err(error).context("asset pack failed; the previous output was restored");
        }
        bail!(
            "asset pack failed: {error:#}; rollback also failed: {}",
            rollback_errors.join("; ")
        );
    }
    Ok(())
}

fn rollback_pack<F>(
    output: &Path,
    moved_to_backup: &[(PathBuf, PathBuf)],
    published: &[PathBuf],
    created_directories: &[PathBuf],
    output_created: bool,
    rename: &mut F,
) -> Vec<String>
where
    F: FnMut(&Path, &Path) -> io::Result<()>,
{
    let mut errors = Vec::new();
    for path in published.iter().rev() {
        if let Err(error) = fs::remove_file(path) {
            if error.kind() != io::ErrorKind::NotFound {
                errors.push(format!("could not remove {}: {error}", path.display()));
            }
        }
    }
    for (backup, destination) in moved_to_backup.iter().rev() {
        if let Some(parent) = destination.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            errors.push(format!(
                "could not restore {}: {error}",
                destination.display()
            ));
            continue;
        }
        if let Err(error) = rename(backup, destination) {
            errors.push(format!(
                "could not restore {}: {error}",
                destination.display()
            ));
        }
    }
    for directory in created_directories.iter().rev() {
        if let Err(error) = fs::remove_dir(directory)
            && error.kind() != io::ErrorKind::NotFound
            && error.kind() != io::ErrorKind::DirectoryNotEmpty
        {
            errors.push(format!("could not remove {}: {error}", directory.display()));
        }
    }
    if output_created
        && let Err(error) = fs::remove_dir(output)
        && error.kind() != io::ErrorKind::NotFound
        && error.kind() != io::ErrorKind::DirectoryNotEmpty
    {
        errors.push(format!("could not remove {}: {error}", output.display()));
    }
    errors
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

fn reject_asset_root_output(root: &Path, output: &Path) -> Result<()> {
    for source_root in ASSET_ROOTS {
        if output.starts_with(root.join(source_root)) {
            bail!(
                "asset output {} is inside the project asset root `{source_root}`; choose a build or distribution directory",
                output.display()
            );
        }
    }
    Ok(())
}

fn collect_project_assets(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut files = BTreeMap::new();
    let mut spelling_by_normalized = BTreeMap::new();
    for directory in ASSET_ROOTS {
        let path = root.join(directory);
        match fs::symlink_metadata(&path) {
            Ok(_) => collect_asset_tree(
                root,
                &path,
                &mut files,
                &mut spelling_by_normalized,
                &mut BTreeSet::new(),
            )?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("could not inspect {}", path.display()));
            }
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
    let path = resolve_exact_disk_case(root, relative, world_file)?;
    let canonical = path.canonicalize().with_context(|| {
        format!(
            "{}: could not resolve asset `{relative}`",
            world_file.display()
        )
    })?;
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

fn resolve_exact_disk_case(root: &Path, relative: &str, world_file: &Path) -> Result<PathBuf> {
    let mut current = root.to_path_buf();
    let mut actual_parts = Vec::new();
    let mut case_mismatch = false;
    for part in relative.split('/') {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                bail!(
                    "{}: referenced asset `{relative}` does not exist",
                    world_file.display()
                );
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "{}: could not inspect referenced asset `{relative}`",
                        world_file.display()
                    )
                });
            }
        };
        let mut exact = None;
        let mut case_insensitive = None;
        for entry in entries {
            let entry = entry.with_context(|| {
                format!(
                    "{}: could not inspect referenced asset `{relative}`",
                    world_file.display()
                )
            })?;
            let name = entry.file_name();
            if name == std::ffi::OsStr::new(part) {
                exact = Some(name);
                break;
            }
            if case_insensitive.is_none() && name.to_string_lossy().eq_ignore_ascii_case(part) {
                case_insensitive = Some(name);
            }
        }
        let name = if let Some(exact) = exact {
            exact
        } else if let Some(case_insensitive) = case_insensitive {
            case_mismatch = true;
            case_insensitive
        } else {
            bail!(
                "{}: referenced asset `{relative}` does not exist",
                world_file.display()
            );
        };
        actual_parts.push(name.to_string_lossy().into_owned());
        current.push(name);
    }
    if case_mismatch {
        bail!(
            "{}: referenced asset `{relative}` does not match disk case; did you mean `{}`?",
            world_file.display(),
            actual_parts.join("/")
        );
    }
    Ok(current)
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
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                }
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    Ok(normalized)
}

fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    let mut current = absolute_output_path(path)?;
    let mut missing_suffix = Vec::<OsString>::new();
    loop {
        match fs::canonicalize(&current) {
            Ok(mut canonical) => {
                for component in missing_suffix.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = current.file_name().ok_or_else(|| {
                    anyhow!("could not resolve asset output path: {}", path.display())
                })?;
                missing_suffix.push(name.to_os_string());
                current = current
                    .parent()
                    .ok_or_else(|| {
                        anyhow!("could not resolve asset output path: {}", path.display())
                    })?
                    .to_path_buf();
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("could not resolve asset output path {}", path.display())
                });
            }
        }
    }
}

fn reject_symlink_components(path: &Path, label: &str) -> Result<()> {
    let absolute = absolute_output_path(path)?;
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata)
                if metadata.file_type().is_symlink() && !is_standard_macos_path_alias(&current) =>
            {
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

#[cfg(target_os = "macos")]
fn is_standard_macos_path_alias(path: &Path) -> bool {
    [
        (Path::new("/var"), Path::new("/private/var")),
        (Path::new("/tmp"), Path::new("/private/tmp")),
    ]
    .iter()
    .any(|(alias, target)| {
        path == *alias && fs::canonicalize(alias).is_ok_and(|resolved| resolved == *target)
    })
}

#[cfg(not(target_os = "macos"))]
fn is_standard_macos_path_alias(_path: &Path) -> bool {
    false
}

fn create_safe_parent_directories(
    root: &Path,
    parent: &Path,
    created_directories: &mut Vec<PathBuf>,
) -> Result<()> {
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
                created_directories.push(current.clone());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect {}", current.display()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{pack_project, pack_project_with_rename};
    use std::fs;

    #[test]
    fn failed_publish_rolls_back_files_and_manifest() {
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(project.path().join("assets")).unwrap();
        fs::write(project.path().join("assets/a.bin"), b"old-a").unwrap();
        fs::write(project.path().join("assets/b.bin"), b"old-b").unwrap();
        let output = tempfile::tempdir().unwrap();
        pack_project(project.path(), output.path()).unwrap();
        fs::write(output.path().join("notes.txt"), b"unrelated user file").unwrap();
        let previous_manifest = fs::read(output.path().join("assets.manifest.json")).unwrap();

        fs::write(project.path().join("assets/a.bin"), b"new-a").unwrap();
        fs::write(project.path().join("assets/b.bin"), b"new-b").unwrap();
        let mut rename_attempt = 0;
        let mut failed_once = false;
        let error =
            pack_project_with_rename(project.path(), output.path(), |source, destination| {
                rename_attempt += 1;
                if rename_attempt == 5 && !failed_once {
                    failed_once = true;
                    return Err(std::io::Error::other("injected publish failure"));
                }
                fs::rename(source, destination)
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("previous output was restored"), "{error}");
        assert!(failed_once);
        assert_eq!(
            fs::read(output.path().join("assets/a.bin")).unwrap(),
            b"old-a"
        );
        assert_eq!(
            fs::read(output.path().join("assets/b.bin")).unwrap(),
            b"old-b"
        );
        assert_eq!(
            fs::read(output.path().join("assets.manifest.json")).unwrap(),
            previous_manifest
        );
        assert_eq!(
            fs::read(output.path().join("notes.txt")).unwrap(),
            b"unrelated user file"
        );
    }
}
