use crate::engine_package::{BORNENGINE_PACKAGE, LEGACY_ENGINE_PACKAGE, is_engine_package};
use crate::package_manager::PackageManager;
use anyhow::{Context, Result, bail};
use semver::Version;
use serde_json::Value;
use std::cmp::Ordering;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineRelease {
    pub package_name: String,
    pub version: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineDependency {
    pub package_name: String,
    pub spec: String,
}

pub fn select_engine_release(
    bornengine_metadata: Option<&Value>,
    legacy_metadata: Option<&Value>,
    requested: Option<&str>,
) -> Result<EngineRelease> {
    for (package_name, metadata) in [
        (BORNENGINE_PACKAGE, bornengine_metadata),
        (LEGACY_ENGINE_PACKAGE, legacy_metadata),
    ] {
        let Some(metadata) = metadata else {
            continue;
        };
        if let Some(version) = select_version(metadata, requested)? {
            return Ok(EngineRelease {
                package_name: package_name.to_owned(),
                version,
            });
        }
    }
    if let Some(version) = requested {
        bail!("BornEngine release `{version}` was not found in the npm registry");
    }
    bail!("no stable BornEngine release was found in the npm registry")
}

fn select_version(metadata: &Value, requested: Option<&str>) -> Result<Option<String>> {
    let versions = metadata
        .get("versions")
        .and_then(Value::as_object)
        .context("npm registry metadata does not contain a versions object")?;
    if let Some(requested) = requested.filter(|requested| *requested != "latest") {
        let version = Version::parse(requested).with_context(|| {
            format!("engine version must be an exact semantic version, got `{requested}`")
        })?;
        if !version.pre.is_empty() || !version.build.is_empty() {
            bail!(
                "BornEngine version must be a stable release without prerelease or build metadata"
            );
        }
        return Ok(versions
            .contains_key(requested)
            .then(|| requested.to_owned()));
    }

    let tagged = metadata
        .pointer("/dist-tags/latest")
        .and_then(Value::as_str)
        .and_then(|tag| Version::parse(tag).ok())
        .filter(|version| version.pre.is_empty() && version.build.is_empty())
        .filter(|version| versions.contains_key(&version.to_string()));
    if let Some(version) = tagged {
        return Ok(Some(version.to_string()));
    }

    let latest = versions
        .keys()
        .filter_map(|version| Version::parse(version).ok())
        .filter(|version| version.pre.is_empty() && version.build.is_empty())
        .max_by(|left, right| left.cmp(right));
    Ok(latest.map(|version| version.to_string()))
}

pub fn resolve_engine_release(requested: Option<&str>) -> Result<EngineRelease> {
    let bornengine = fetch_registry_metadata(BORNENGINE_PACKAGE)
        .with_context(|| format!("could not query npm metadata for {BORNENGINE_PACKAGE}"))?;
    let legacy = fetch_registry_metadata(LEGACY_ENGINE_PACKAGE)
        .with_context(|| format!("could not query npm metadata for {LEGACY_ENGINE_PACKAGE}"))?;
    select_engine_release(bornengine.as_ref(), legacy.as_ref(), requested)
}

pub fn list_engine_releases() -> Result<Vec<EngineRelease>> {
    let bornengine = fetch_registry_metadata(BORNENGINE_PACKAGE)
        .with_context(|| format!("could not query npm metadata for {BORNENGINE_PACKAGE}"))?;
    if let Some(metadata) = bornengine.as_ref() {
        let releases = stable_releases(metadata, BORNENGINE_PACKAGE)?;
        if !releases.is_empty() {
            return Ok(releases);
        }
    }
    let legacy = fetch_registry_metadata(LEGACY_ENGINE_PACKAGE)
        .with_context(|| format!("could not query npm metadata for {LEGACY_ENGINE_PACKAGE}"))?;
    let Some(metadata) = legacy else {
        bail!("no BornEngine package versions were found in the npm registry");
    };
    stable_releases(&metadata, LEGACY_ENGINE_PACKAGE)
}

fn stable_releases(metadata: &Value, package_name: &str) -> Result<Vec<EngineRelease>> {
    let versions = metadata
        .get("versions")
        .and_then(Value::as_object)
        .context("npm registry metadata does not contain a versions object")?;
    let mut versions = versions
        .keys()
        .filter_map(|version| Version::parse(version).ok())
        .filter(|version| version.pre.is_empty() && version.build.is_empty())
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| right.cmp(left));
    Ok(versions
        .into_iter()
        .map(|version| EngineRelease {
            package_name: package_name.to_owned(),
            version: version.to_string(),
        })
        .collect())
}

fn fetch_registry_metadata(package_name: &str) -> Result<Option<Value>> {
    let escaped_name = package_name.replace('/', "%2f");
    let url = format!("https://registry.npmjs.org/{escaped_name}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let response = match agent.get(&url).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(error) => return Err(error).context("npm registry request failed"),
    };
    response
        .into_body()
        .read_json::<Value>()
        .map(Some)
        .context("npm registry returned invalid package metadata")
}

pub fn engine_package_from_manifest(package: &Value) -> Result<String> {
    let name = package
        .get("name")
        .and_then(Value::as_str)
        .context("engine package.json is missing a string `name`")?;
    if !is_engine_package(name) {
        bail!("local package `{name}` is not named `@bornengine/engine` or `@bloomengine/engine`");
    }
    Ok(name.to_owned())
}

pub fn local_engine_dependency(
    engine_path: &Path,
    project_root: &Path,
    package_manager: PackageManager,
) -> Result<EngineDependency> {
    let engine_root = engine_path.canonicalize().with_context(|| {
        format!(
            "local engine path does not exist: {}",
            engine_path.display()
        )
    })?;
    if !engine_root.is_dir() {
        bail!(
            "local engine path is not a directory: {}",
            engine_root.display()
        );
    }
    let package_path = engine_root.join("package.json");
    let package_contents = std::fs::read_to_string(&package_path)
        .with_context(|| format!("could not read {}", package_path.display()))?;
    let package: Value = serde_json::from_str(&package_contents)
        .with_context(|| format!("invalid JSON in {}", package_path.display()))?;
    let package_name = engine_package_from_manifest(&package)?;
    let project_root = absolute_project_root(project_root)?;
    let relative = relative_path(&project_root, &engine_root)?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    let prefix = if package_manager == PackageManager::Pnpm {
        "link:"
    } else {
        "file:"
    };
    Ok(EngineDependency {
        package_name,
        spec: format!("{prefix}{relative}"),
    })
}

fn absolute_project_root(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return path
            .canonicalize()
            .with_context(|| format!("could not resolve project directory {}", path.display()));
    }
    let parent = path
        .parent()
        .context("project directory has no parent")?
        .canonicalize()
        .with_context(|| format!("could not resolve project parent of {}", path.display()))?;
    let file_name = path
        .file_name()
        .context("project directory has no final path component")?;
    Ok(parent.join(file_name))
}

fn relative_path(from: &Path, to: &Path) -> Result<PathBuf> {
    let from_components = from.components().collect::<Vec<_>>();
    let to_components = to.components().collect::<Vec<_>>();
    let common = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0
        || !matches!(
            from_components.first(),
            Some(Component::RootDir | Component::Prefix(_))
        )
    {
        bail!("local engine path and project must be on the same filesystem root");
    }
    let mut relative = PathBuf::new();
    for component in from_components.iter().skip(common) {
        if !matches!(component, Component::RootDir | Component::Prefix(_)) {
            relative.push("..");
        }
    }
    for component in to_components.iter().skip(common) {
        relative.push(component.as_os_str());
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Ok(relative)
}

pub fn engine_dependency(package: &Value) -> Result<Option<EngineDependency>> {
    for section in ["dependencies", "devDependencies", "optionalDependencies"] {
        let Some(dependencies) = package.get(section).and_then(Value::as_object) else {
            continue;
        };
        for (name, spec) in dependencies {
            if is_engine_package(name) {
                let spec = spec.as_str().with_context(|| {
                    format!("engine dependency `{name}` must have a string value")
                })?;
                return Ok(Some(EngineDependency {
                    package_name: name.clone(),
                    spec: spec.to_owned(),
                }));
            }
        }
    }
    Ok(None)
}

pub fn set_engine_dependency(package: &mut Value, dependency: &EngineDependency) -> Result<()> {
    if !is_engine_package(&dependency.package_name) {
        bail!(
            "unsupported BornEngine package name `{}`",
            dependency.package_name
        );
    }
    let package_object = package
        .as_object_mut()
        .context("project package.json must contain a JSON object")?;
    for section in ["dependencies", "devDependencies", "optionalDependencies"] {
        if let Some(dependencies) = package_object
            .get_mut(section)
            .and_then(Value::as_object_mut)
        {
            dependencies.retain(|name, _| !is_engine_package(name));
        }
    }
    let dependencies = package_object
        .entry("dependencies")
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .context("project `dependencies` must be an object")?;
    dependencies.insert(
        dependency.package_name.clone(),
        Value::String(dependency.spec.clone()),
    );
    Ok(())
}

pub fn remove_engine_dependency(package: &mut Value) -> Result<Option<EngineDependency>> {
    let previous = engine_dependency(package)?;
    let package_object = package
        .as_object_mut()
        .context("project package.json must contain a JSON object")?;
    for section in ["dependencies", "devDependencies", "optionalDependencies"] {
        if let Some(dependencies) = package_object
            .get_mut(section)
            .and_then(Value::as_object_mut)
        {
            dependencies.retain(|name, _| !is_engine_package(name));
        }
    }
    Ok(previous)
}

pub fn compare_versions(left: &str, right: &str) -> Result<Ordering> {
    Ok(Version::parse(left)?.cmp(&Version::parse(right)?))
}
