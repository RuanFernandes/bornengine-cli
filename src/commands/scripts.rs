use anyhow::{Context, Result, bail};
use boa_ast::{
    ModuleItem,
    declaration::ExportDeclaration,
    expression::ImportCall,
    scope::Scope,
    visitor::{VisitWith, Visitor},
};
use boa_interner::Interner;
use boa_parser::{Parser, Source};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read};
use std::ops::ControlFlow;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MANIFEST_NAME: &str = "bornengine.script.json";
const MANIFEST_FORMAT: &str = "bornengine-script-v1";
const API_VERSION: u32 = 1;
const MAX_SOURCE_BYTES: u64 = 1024 * 1024;
const OWNERSHIP_NAME: &str = ".bornengine-pack.json";
const ALLOWED_PERMISSIONS: [&str; 4] = [
    "log",
    "self.particles.emit",
    "self.read",
    "self.transform.write",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScriptPackageManifest {
    pub format: String,
    pub api_version: u32,
    pub entry: String,
    pub permissions: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScriptPackSummary {
    pub files: usize,
    pub bytes: u64,
    pub entry: String,
}

struct ScriptPackage {
    manifest: ScriptPackageManifest,
    root: PathBuf,
    entry_bytes: Vec<u8>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PackOwnership {
    format: String,
    manifest_sha256: String,
    entry_sha256: String,
}

pub fn check_package(manifest_path: Option<&Path>) -> Result<ScriptPackageManifest> {
    let package = read_package(manifest_path)?;
    Ok(package.manifest)
}

pub fn pack_package(
    manifest_path: Option<&Path>,
    output_directory: &Path,
) -> Result<ScriptPackSummary> {
    let package = read_package(manifest_path)?;
    let entry_bytes = &package.entry_bytes;

    let requested_output = absolute_path(output_directory)?;
    reject_symlink_components(&requested_output, "script output")?;
    let output_parent = requested_output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&output_parent)
        .with_context(|| format!("could not create output parent {}", output_parent.display()))?;
    reject_symlink_components(&output_parent, "script output parent")?;

    let output_parent = fs::canonicalize(&output_parent).with_context(|| {
        format!(
            "could not resolve output parent {}",
            output_parent.display()
        )
    })?;
    let output_name = requested_output
        .file_name()
        .filter(|name| !name.is_empty())
        .context("script output must name a directory")?;
    let output = output_parent.join(output_name);
    reject_symlink_components(&output, "script output")?;
    if output == package.root || package.root.starts_with(&output) {
        bail!("script output must not replace the package root or one of its parents");
    }

    let output_exists = validate_output_directory(&output)?;
    if output_exists {
        validate_previous_pack(&output)?;
    }

    let mut stage = ScriptPackStage::create(&output_parent)?;
    let staged_files = stage.path.join("next");
    fs::create_dir(&staged_files).with_context(|| {
        format!(
            "could not create staging directory {}",
            staged_files.display()
        )
    })?;
    let staged_entry = staged_files.join(Path::new(&package.manifest.entry));
    let staged_entry_parent = staged_entry
        .parent()
        .context("script entry path has no parent directory")?;
    fs::create_dir_all(staged_entry_parent).with_context(|| {
        format!(
            "could not create staging directory {}",
            staged_entry_parent.display()
        )
    })?;
    fs::write(&staged_entry, entry_bytes)
        .with_context(|| format!("could not stage script entry `{}`", package.manifest.entry))?;

    let mut manifest_bytes = serde_json::to_vec_pretty(&package.manifest)?;
    manifest_bytes.push(b'\n');
    let manifest_digest = sha256(&manifest_bytes);
    let manifest_bytes_len = manifest_bytes.len();
    fs::write(staged_files.join(MANIFEST_NAME), manifest_bytes)
        .context("could not stage script manifest")?;
    let marker = PackOwnership {
        format: "bornengine-cli-script-pack-v1".into(),
        manifest_sha256: manifest_digest,
        entry_sha256: sha256(entry_bytes),
    };
    let mut marker_bytes = serde_json::to_vec_pretty(&marker)?;
    marker_bytes.push(b'\n');
    let marker_bytes_len = marker_bytes.len();
    fs::write(staged_files.join(OWNERSHIP_NAME), marker_bytes)
        .context("could not stage script pack ownership marker")?;

    publish_pack(&output, &mut stage, &staged_files, output_exists)?;
    Ok(ScriptPackSummary {
        files: 3,
        bytes: (entry_bytes.len() + manifest_bytes_len + marker_bytes_len) as u64,
        entry: package.manifest.entry,
    })
}

fn read_package(manifest_path: Option<&Path>) -> Result<ScriptPackage> {
    let manifest_input = manifest_path.unwrap_or_else(|| Path::new(MANIFEST_NAME));
    let manifest_path = absolute_path(manifest_input)?;
    reject_symlink_components(&manifest_path, "script manifest")?;
    let metadata = fs::symlink_metadata(&manifest_path).with_context(|| {
        format!(
            "could not inspect script manifest {}",
            manifest_path.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "script manifest must be a regular file: {}",
            manifest_path.display()
        );
    }

    let manifest_path = fs::canonicalize(&manifest_path).with_context(|| {
        format!(
            "could not resolve script manifest {}",
            manifest_path.display()
        )
    })?;
    let root = manifest_path
        .parent()
        .context("script manifest has no package directory")?
        .to_path_buf();
    let bytes = fs::read(&manifest_path)
        .with_context(|| format!("could not read script manifest {}", manifest_path.display()))?;
    let manifest: ScriptPackageManifest = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid script manifest {}", manifest_path.display()))?;
    validate_manifest(&manifest)?;
    let entry_path = resolve_regular_entry(&root, &manifest.entry)?;
    let entry_bytes = read_bounded_entry(&entry_path, &manifest.entry)?;
    validate_source(&entry_bytes)?;
    Ok(ScriptPackage {
        manifest,
        root,
        entry_bytes,
    })
}

fn read_bounded_entry(path: &Path, entry: &str) -> Result<Vec<u8>> {
    let metadata =
        fs::metadata(path).with_context(|| format!("could not inspect script entry `{entry}`"))?;
    if metadata.len() > MAX_SOURCE_BYTES {
        bail!("script entry `{entry}` exceeds the 1048576 byte limit");
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .with_context(|| format!("could not read script entry `{entry}`"))?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("could not read script entry `{entry}`"))?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        bail!("script entry `{entry}` exceeds the 1048576 byte limit");
    }
    Ok(bytes)
}

fn validate_source(bytes: &[u8]) -> Result<()> {
    let source = std::str::from_utf8(bytes).context("script entry must be UTF-8")?;
    let mut parser = Parser::new(Source::from_bytes(source));
    let mut interner = Interner::default();
    let module = parser
        .parse_module(&Scope::new_global(), &mut interner)
        .map_err(|error| anyhow::anyhow!("script entry has invalid JavaScript syntax: {error}"))?;
    for item in module.items().items() {
        match item {
            ModuleItem::ImportDeclaration(_)
            | ModuleItem::ExportDeclaration(ExportDeclaration::ReExport { .. }) => {
                bail!(
                    "script entry uses a static import or re-export, but v1 has no module loader"
                );
            }
            _ => {}
        }
    }
    struct DynamicImportFinder;
    impl<'ast> Visitor<'ast> for DynamicImportFinder {
        type BreakTy = ();

        fn visit_import_call(&mut self, _: &'ast ImportCall) -> ControlFlow<Self::BreakTy> {
            ControlFlow::Break(())
        }
    }
    if module.visit_with(&mut DynamicImportFinder).is_break() {
        bail!("script entry uses dynamic import, but v1 has no module loader");
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_manifest(manifest: &ScriptPackageManifest) -> Result<()> {
    if manifest.format != MANIFEST_FORMAT {
        bail!("unrecognized script package format `{}`", manifest.format);
    }
    if manifest.api_version != API_VERSION {
        bail!(
            "unsupported script API version {}; expected {API_VERSION}",
            manifest.api_version
        );
    }
    validate_entry_name(&manifest.entry)?;

    for permission in &manifest.permissions {
        if !ALLOWED_PERMISSIONS.contains(&permission.as_str()) {
            bail!("unknown script permission `{permission}`");
        }
    }
    if manifest
        .permissions
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        bail!("script permissions must be sorted and unique");
    }
    Ok(())
}

fn validate_entry_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('/')
        || value.contains('\\')
        || value.contains(':')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("unsafe script entry path `{value}`; use a normalized package-relative path");
    }

    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("unsafe script entry path `{value}`; use a normalized package-relative path");
    }

    match path.extension().and_then(|extension| extension.to_str()) {
        Some("js" | "mjs") => Ok(()),
        _ => bail!("script entry `{value}` must have a .js or .mjs extension"),
    }
}

fn resolve_regular_entry(root: &Path, relative: &str) -> Result<PathBuf> {
    validate_entry_name(relative)?;
    let mut current = root.to_path_buf();
    let components = Path::new(relative).components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)
            .with_context(|| format!("script entry `{relative}` does not exist"))?;
        if metadata.file_type().is_symlink() {
            bail!("script entry `{relative}` must not contain symbolic links");
        }
        let final_component = index + 1 == components.len();
        if final_component && !metadata.is_file() {
            bail!("script entry `{relative}` is not a regular file");
        }
        if !final_component && !metadata.is_dir() {
            bail!("script entry parent in `{relative}` is not a directory");
        }
    }
    let canonical = fs::canonicalize(&current)
        .with_context(|| format!("could not resolve script entry `{relative}`"))?;
    if !canonical.starts_with(root) {
        bail!("script entry `{relative}` resolves outside its package root");
    }
    Ok(canonical)
}

fn validate_output_directory(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "refusing to use a symbolic link as script output: {}",
                path.display()
            );
        }
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => bail!(
            "script output exists and is not a directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
    }
}

fn validate_previous_pack(output: &Path) -> Result<()> {
    let (files, directories) = collect_output_inventory(output)?;
    if files.is_empty() && directories.is_empty() {
        return Ok(());
    }

    let marker_path = output.join(OWNERSHIP_NAME);
    let marker_metadata = fs::symlink_metadata(&marker_path)
        .context("refusing to replace script output without a CLI ownership marker")?;
    if !marker_metadata.is_file() || marker_metadata.file_type().is_symlink() {
        bail!("existing script output ownership marker is not a regular file");
    }
    let marker: PackOwnership = serde_json::from_slice(&fs::read(&marker_path)?)
        .context("existing script output ownership marker is invalid")?;
    if marker.format != "bornengine-cli-script-pack-v1" {
        bail!("existing script output ownership marker has an unknown format");
    }

    let manifest_path = output.join(MANIFEST_NAME);
    match fs::symlink_metadata(&manifest_path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => bail!("existing script output manifest is not a regular file"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            bail!("refusing to replace script output containing files not owned by BornEngine")
        }
        Err(error) => {
            return Err(error).context("could not inspect existing script output manifest");
        }
    };
    let bytes =
        fs::read(&manifest_path).context("could not read existing script output manifest")?;
    if marker.manifest_sha256 != sha256(&bytes) {
        bail!("refusing to replace modified script output manifest");
    }
    let previous: ScriptPackageManifest = serde_json::from_slice(&bytes)
        .context("existing script output has an invalid BornEngine manifest")?;
    validate_manifest(&previous).context("existing script output manifest is invalid")?;
    let previous_entry = resolve_regular_entry(output, &previous.entry)
        .context("existing script output entry is invalid")?;
    let previous_bytes = read_bounded_entry(&previous_entry, &previous.entry)?;
    if marker.entry_sha256 != sha256(&previous_bytes) {
        bail!("refusing to replace modified script output entry");
    }

    let expected_files = BTreeSet::from([
        OWNERSHIP_NAME.to_owned(),
        MANIFEST_NAME.to_owned(),
        previous.entry.clone(),
    ]);
    let mut expected_directories = BTreeSet::new();
    let mut parent = Path::new(&previous.entry).parent();
    while let Some(directory) = parent {
        if directory.as_os_str().is_empty() {
            break;
        }
        expected_directories.insert(path_string(directory));
        parent = directory.parent();
    }
    if files != expected_files || directories != expected_directories {
        bail!("refusing to replace script output with untracked or missing content");
    }
    Ok(())
}

fn collect_output_inventory(root: &Path) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut BTreeSet<String>,
        directories: &mut BTreeSet<String>,
    ) -> Result<()> {
        let mut entries = fs::read_dir(directory)
            .with_context(|| format!("could not inspect script output {}", directory.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort();
        for path in entries {
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("could not inspect script output {}", path.display()))?;
            if metadata.file_type().is_symlink() {
                bail!("refusing to replace script output containing symbolic links");
            }
            let relative = path
                .strip_prefix(root)
                .context("script output inventory escaped its root")?;
            let name = path_string(relative);
            if metadata.is_dir() {
                directories.insert(name);
                visit(root, &path, files, directories)?;
            } else if metadata.is_file() {
                files.insert(name);
            } else {
                bail!("refusing to replace script output containing a non-regular file");
            }
        }
        Ok(())
    }

    let mut files = BTreeSet::new();
    let mut directories = BTreeSet::new();
    visit(root, root, &mut files, &mut directories)?;
    Ok((files, directories))
}

fn publish_pack(
    output: &Path,
    stage: &mut ScriptPackStage,
    staged_files: &Path,
    output_exists: bool,
) -> Result<()> {
    let backup = stage.path.join("previous");
    if output_exists {
        fs::rename(output, &backup).with_context(|| {
            format!(
                "could not stage existing script output {}",
                output.display()
            )
        })?;
    }
    if let Err(publish_error) = fs::rename(staged_files, output) {
        if output_exists {
            if let Err(restore_error) = fs::rename(&backup, output) {
                stage.preserve();
                bail!(
                    "script pack failed: {publish_error}; previous output is preserved at {} because rollback failed: {restore_error}",
                    backup.display()
                );
            }
        }
        return Err(publish_error)
            .with_context(|| format!("could not publish script output {}", output.display()));
    }
    Ok(())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn reject_symlink_components(path: &Path, label: &str) -> Result<()> {
    let absolute = absolute_path(path)?;
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "refusing symbolic link in {label} path {}",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect {}", current.display()));
            }
        }
    }
    Ok(())
}

struct ScriptPackStage {
    path: PathBuf,
    preserve_on_drop: bool,
}

impl ScriptPackStage {
    fn create(parent: &Path) -> Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for attempt in 0_u32..u32::MAX {
            let candidate = parent.join(format!(
                ".bornengine-script-pack-{}-{timestamp}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => {
                    return Ok(Self {
                        path: candidate,
                        preserve_on_drop: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "could not create script staging directory {}",
                            candidate.display()
                        )
                    });
                }
            }
        }
        bail!("could not allocate a unique script staging directory")
    }

    fn preserve(&mut self) {
        // On a failed rollback, keep the previous package where it can be recovered.
        self.preserve_on_drop = true;
    }
}

impl Drop for ScriptPackStage {
    fn drop(&mut self) {
        if !self.preserve_on_drop {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
