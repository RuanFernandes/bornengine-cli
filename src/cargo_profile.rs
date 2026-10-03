use crate::process::executable_path_in_path;
use crate::project::GameKind;
use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const PROXY_ENGINE_ROOT: &str = "BORNENGINE_CARGO_PROXY_ENGINE_ROOT";
const PROXY_REAL_CARGO: &str = "BORNENGINE_CARGO_PROXY_REAL";
const PROXY_FEATURES: &str = "BORNENGINE_CARGO_PROXY_FEATURES";
const CARGO_TARGET_DIR: &str = "CARGO_TARGET_DIR";

pub fn effective_cargo_target_dir_from(base_dir: &Path) -> Result<PathBuf> {
    let cache_dir = BaseDirs::new().map(|directories| directories.cache_dir().to_path_buf());
    resolve_cargo_target_dir_from(
        cache_dir.as_deref(),
        std::env::var_os(CARGO_TARGET_DIR),
        base_dir,
    )
}

pub fn cargo_target_dir_environment_from(base_dir: &Path) -> Result<Vec<(OsString, OsString)>> {
    let cache_dir = BaseDirs::new().map(|directories| directories.cache_dir().to_path_buf());
    cargo_target_dir_env_from(
        cache_dir.as_deref(),
        std::env::var_os(CARGO_TARGET_DIR),
        base_dir,
    )
}

fn cargo_target_dir_env_from(
    cache_dir: Option<&Path>,
    override_dir: Option<OsString>,
    base_dir: &Path,
) -> Result<Vec<(OsString, OsString)>> {
    let Some(override_dir) = override_dir else {
        return cargo_target_dir_env(cache_dir, None);
    };
    if Path::new(&override_dir).is_absolute() {
        return Ok(Vec::new());
    }
    let target_dir = resolve_cargo_target_dir_from(cache_dir, Some(override_dir), base_dir)?;
    Ok(vec![(CARGO_TARGET_DIR.into(), target_dir.into_os_string())])
}

pub fn native_build_environment(
    development: bool,
    jobs: Option<usize>,
) -> Vec<(OsString, OsString)> {
    let mut environment = Vec::new();
    if development {
        environment.push(("CARGO_PROFILE_DEV_OPT_LEVEL".into(), "1".into()));
        environment.push(("CARGO_INCREMENTAL".into(), "1".into()));
    }
    if let Some(jobs) = jobs {
        environment.push(("CARGO_BUILD_JOBS".into(), jobs.to_string().into()));
    }
    environment
}

fn resolve_cargo_target_dir(
    cache_dir: Option<&Path>,
    override_dir: Option<OsString>,
) -> Result<PathBuf> {
    if let Some(override_dir) = override_dir {
        return Ok(PathBuf::from(override_dir));
    }
    let cache_dir = cache_dir.context(
        "could not determine the user cache directory; set CARGO_TARGET_DIR to choose a cache path",
    )?;
    Ok(cache_dir.join("BornEngine").join("cargo-target"))
}

fn resolve_cargo_target_dir_from(
    cache_dir: Option<&Path>,
    override_dir: Option<OsString>,
    base_dir: &Path,
) -> Result<PathBuf> {
    let target_dir = resolve_cargo_target_dir(cache_dir, override_dir)?;
    if target_dir.is_absolute() {
        return Ok(target_dir);
    }
    let base_dir = if base_dir.is_absolute() {
        base_dir.to_path_buf()
    } else {
        std::env::current_dir()
            .context("could not determine current directory")?
            .join(base_dir)
    };
    Ok(base_dir.join(target_dir))
}

fn cargo_target_dir_env(
    cache_dir: Option<&Path>,
    override_dir: Option<OsString>,
) -> Result<Vec<(OsString, OsString)>> {
    if override_dir.is_some() {
        return Ok(Vec::new());
    }
    let target_dir = resolve_cargo_target_dir(cache_dir, None)?;
    Ok(vec![(CARGO_TARGET_DIR.into(), target_dir.into_os_string())])
}

pub fn read_native_profile(project_root: &Path) -> Result<GameKind> {
    let (path, config) = read_project_config(project_root)?;
    let Some(config) = config else {
        return Ok(GameKind::TwoD);
    };
    native_profile_from_config(&config, &path)
}

pub fn read_native_features(project_root: &Path) -> Result<Vec<String>> {
    let (path, config) = read_project_config(project_root)?;
    let Some(config) = config else {
        return Ok(GameKind::TwoD
            .native_features()
            .iter()
            .map(|feature| (*feature).to_owned())
            .collect());
    };
    let profile = native_profile_from_config(&config, &path)?;
    let mut features = profile
        .native_features()
        .iter()
        .map(|feature| (*feature).to_owned())
        .collect::<Vec<_>>();
    if let Some(extra) = config
        .get("bornengine")
        .and_then(|section| section.get("native_features"))
    {
        let extra = extra
            .as_array()
            .context("`bornengine.native_features` must be an array")?;
        for feature in extra {
            let feature = feature
                .as_str()
                .context("each `bornengine.native_features` entry must be a string")?;
            if feature.is_empty()
                || !feature.chars().all(|character| {
                    character.is_ascii_alphanumeric() || character == '-' || character == '_'
                })
            {
                bail!(
                    "invalid BornEngine native feature `{feature}` in {}",
                    path.display()
                );
            }
            if !features.iter().any(|existing| existing == feature) {
                features.push(feature.to_owned());
            }
        }
    }
    let has_explicit_profile = config
        .get("bornengine")
        .and_then(|section| section.get("native_profile"))
        .is_some();
    if !has_explicit_profile {
        if let Some(legacy) = config
            .get("native-library")
            .and_then(|section| section.get("@bornengine/engine"))
            .and_then(|library| library.get("features"))
            .and_then(toml::Value::as_array)
        {
            for feature in legacy.iter().filter_map(toml::Value::as_str) {
                if !features.iter().any(|existing| existing == feature) {
                    features.push(feature.to_owned());
                }
            }
        }
    }
    Ok(features)
}

fn read_project_config(project_root: &Path) -> Result<(PathBuf, Option<toml::Value>)> {
    let path = project_root.join("perry.toml");
    if !path.is_file() {
        return Ok((path, None));
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("could not read BornEngine profile at {}", path.display()))?;
    let config = toml::from_str::<toml::Value>(&contents)
        .with_context(|| format!("invalid TOML in {}", path.display()))?;
    Ok((path, Some(config)))
}

fn native_profile_from_config(config: &toml::Value, path: &Path) -> Result<GameKind> {
    if let Some(value) = config
        .get("bornengine")
        .and_then(|section| section.get("native_profile"))
    {
        let profile = value
            .as_str()
            .context("`bornengine.native_profile` must be a string")?;
        return GameKind::from_native_profile(profile).with_context(|| {
            format!(
                "unknown BornEngine native profile `{profile}` in {}",
                path.display()
            )
        });
    }

    // Preserve profiles generated by early BornEngine CLI versions.
    if let Some(features) = config
        .get("native-library")
        .and_then(|section| section.get("@bornengine/engine"))
        .and_then(|library| library.get("features"))
        .and_then(toml::Value::as_array)
    {
        let has_feature = |name: &str| {
            features
                .iter()
                .any(|feature| feature.as_str() == Some(name))
        };
        return Ok(if has_feature("jolt") {
            GameKind::ThreeD
        } else if has_feature("models3d") {
            GameKind::TwoPointFiveD
        } else {
            GameKind::TwoD
        });
    }
    Ok(GameKind::TwoD)
}

pub fn args_with_profile(
    args: &[OsString],
    engine_root: &Path,
    features: &[&str],
) -> Vec<OsString> {
    let mut adjusted = args.to_vec();
    let is_build = args.iter().any(|arg| arg == OsStr::new("build"));
    let has_feature_override = args.iter().any(|arg| {
        matches!(
            arg.to_string_lossy().as_ref(),
            "--no-default-features" | "--all-features" | "--features" | "-F"
        ) || arg.to_string_lossy().starts_with("--features=")
    });
    if !is_build || has_feature_override {
        return adjusted;
    }

    let manifest = manifest_path_arg(args);
    let Some(manifest) = manifest else {
        return adjusted;
    };
    if !is_engine_manifest(manifest, engine_root) {
        return adjusted;
    }

    adjusted.push(OsString::from("--no-default-features"));
    if !features.is_empty() {
        adjusted.push(OsString::from("--features"));
        adjusted.push(OsString::from(features.join(",")));
    }
    adjusted
}

fn manifest_path_arg(args: &[OsString]) -> Option<&Path> {
    args.iter().enumerate().find_map(|(index, arg)| {
        if arg == OsStr::new("--manifest-path") {
            args.get(index + 1).map(Path::new)
        } else {
            arg.to_str()
                .and_then(|arg| arg.strip_prefix("--manifest-path="))
                .map(Path::new)
        }
    })
}

fn is_engine_manifest(manifest: &Path, engine_root: &Path) -> bool {
    let Ok(engine_root) = engine_root.canonicalize() else {
        return false;
    };
    let manifest = if manifest.is_absolute() {
        manifest.to_path_buf()
    } else if let Ok(current_dir) = std::env::current_dir() {
        current_dir.join(manifest)
    } else {
        return false;
    };
    let Ok(manifest) = manifest.canonicalize() else {
        return false;
    };
    let Some(target_directory) = manifest.parent() else {
        return false;
    };
    let Some(native_directory) = target_directory.parent() else {
        return false;
    };
    let target = target_directory.file_name().and_then(OsStr::to_str);
    manifest.file_name() == Some(OsStr::new("Cargo.toml"))
        && native_directory.file_name() == Some(OsStr::new("native"))
        && native_directory.parent() == Some(engine_root.as_path())
        && matches!(
            target,
            Some(
                "android" | "ios" | "linux" | "macos" | "tvos" | "visionos" | "watchos" | "windows"
            )
        )
}

pub fn run_cargo_proxy_if_requested() -> Option<i32> {
    let engine_root = std::env::var_os(PROXY_ENGINE_ROOT)?;
    let Some(real_cargo) = std::env::var_os(PROXY_REAL_CARGO) else {
        eprintln!("BornEngine Cargo profile wrapper is missing its Cargo executable path");
        return Some(1);
    };
    let features = std::env::var_os(PROXY_FEATURES)
        .unwrap_or_default()
        .to_string_lossy()
        .split(',')
        .filter(|feature| !feature.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let feature_refs = features.iter().map(String::as_str).collect::<Vec<_>>();
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let args = args_with_profile(&args, Path::new(&engine_root), &feature_refs);
    match Command::new(real_cargo).args(&args).status() {
        Ok(status) => {
            let exit_code = status.code().unwrap_or(1);
            if status.success() {
                let expose_result = cargo_target_directory(&args).and_then(|target_directory| {
                    mirror_engine_staticlib_for_perry(
                        &args,
                        Path::new(&engine_root),
                        &target_directory,
                    )
                });
                if let Err(error) = expose_result {
                    eprintln!("could not expose the shared BornEngine library to Perry: {error:#}");
                    return Some(1);
                }
            }
            Some(exit_code)
        }
        Err(error) => {
            eprintln!("could not start Cargo for the BornEngine build: {error}");
            Some(1)
        }
    }
}

fn cargo_target_directory(args: &[OsString]) -> Result<PathBuf> {
    if let Some(directory) = option_path_arg(args, "--target-dir") {
        return Ok(directory.to_path_buf());
    }
    if let Some(directory) = std::env::var_os(CARGO_TARGET_DIR) {
        return Ok(PathBuf::from(directory));
    }
    let cache_dir = BaseDirs::new().map(|directories| directories.cache_dir().to_path_buf());
    resolve_cargo_target_dir(cache_dir.as_deref(), None)
}

fn mirror_engine_staticlib_for_perry(
    args: &[OsString],
    engine_root: &Path,
    cargo_target_dir: &Path,
) -> Result<()> {
    let Some(manifest_arg) = manifest_path_arg(args) else {
        return Ok(());
    };
    let manifest = if manifest_arg.is_absolute() {
        manifest_arg.to_path_buf()
    } else {
        std::env::current_dir()
            .context("could not determine Cargo's working directory")?
            .join(manifest_arg)
    };
    let manifest = manifest
        .canonicalize()
        .with_context(|| format!("could not resolve Cargo manifest {}", manifest.display()))?;
    let engine_root = engine_root
        .canonicalize()
        .with_context(|| format!("could not resolve engine package {}", engine_root.display()))?;
    if !is_engine_manifest(&manifest, &engine_root) {
        return Ok(());
    }

    let manifest_contents = fs::read_to_string(&manifest)
        .with_context(|| format!("could not read engine manifest {}", manifest.display()))?;
    let manifest_config = toml::from_str::<toml::Value>(&manifest_contents)
        .with_context(|| format!("invalid engine manifest {}", manifest.display()))?;
    let Some(library) = manifest_config.get("lib") else {
        return Ok(());
    };
    if !library
        .get("crate-type")
        .and_then(toml::Value::as_array)
        .is_some_and(|types| types.iter().any(|kind| kind.as_str() == Some("staticlib")))
    {
        return Ok(());
    }
    let library_name = library
        .get("name")
        .and_then(toml::Value::as_str)
        .context("BornEngine native manifest is missing its static library name")?;
    if library_name.is_empty()
        || !library_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        bail!(
            "invalid static library name `{library_name}` in {}",
            manifest.display()
        );
    }

    let profile = cargo_profile_directory(args);
    let target = option_string_arg(args, "--target");
    let platform = manifest
        .parent()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        .context("engine manifest has no native platform directory")?;
    let extension = if platform == "windows"
        || target
            .as_deref()
            .is_some_and(|name| name.contains("windows"))
    {
        "lib"
    } else {
        "a"
    };
    let archive_name = if extension == "lib" {
        format!("{library_name}.{extension}")
    } else {
        format!("lib{library_name}.{extension}")
    };
    let mut shared_artifact_directory = cargo_target_dir.to_path_buf();
    if let Some(target) = target.as_deref() {
        shared_artifact_directory.push(target);
    }
    shared_artifact_directory.push(&profile);
    let source = shared_artifact_directory.join(&archive_name);
    if !source.is_file() {
        return Ok(());
    }

    let mut perry_artifact_directory = manifest
        .parent()
        .context("engine manifest has no parent directory")?
        .join("target");
    if let Some(target) = target.as_deref() {
        perry_artifact_directory.push(target);
    }
    perry_artifact_directory.push(profile);
    let destination = perry_artifact_directory.join(archive_name);
    mirror_file(&source, &destination)
}

fn cargo_profile_directory(args: &[OsString]) -> String {
    if args.iter().any(|arg| arg == OsStr::new("--release")) {
        return "release".to_owned();
    }
    option_string_arg(args, "--profile")
        .map(|profile| {
            if profile == "dev" {
                "debug".to_owned()
            } else {
                profile
            }
        })
        .unwrap_or_else(|| "debug".to_owned())
}

fn option_path_arg<'a>(args: &'a [OsString], option: &str) -> Option<&'a Path> {
    args.iter().enumerate().find_map(|(index, arg)| {
        if arg == OsStr::new(option) {
            args.get(index + 1).map(Path::new)
        } else {
            arg.to_str()
                .and_then(|arg| arg.strip_prefix(&format!("{option}=")))
                .map(Path::new)
        }
    })
}

fn option_string_arg(args: &[OsString], option: &str) -> Option<String> {
    args.iter().enumerate().find_map(|(index, arg)| {
        if arg == OsStr::new(option) {
            args.get(index + 1)
                .and_then(|value| value.to_str())
                .map(str::to_owned)
        } else {
            arg.to_str()
                .and_then(|arg| arg.strip_prefix(&format!("{option}=")))
                .map(str::to_owned)
        }
    })
}

fn mirror_file(source: &Path, destination: &Path) -> Result<()> {
    if source == destination {
        return Ok(());
    }
    let parent = destination
        .parent()
        .context("Perry static library path has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "could not create Perry artifact directory {}",
            parent.display()
        )
    })?;
    let filename = destination
        .file_name()
        .and_then(OsStr::to_str)
        .context("Perry static library path has no valid filename")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(
        ".{filename}.bornengine-{}-{nonce}",
        std::process::id()
    ));
    if fs::hard_link(source, &temporary).is_err() {
        fs::copy(source, &temporary).with_context(|| {
            format!(
                "could not link or copy shared Cargo artifact {} to {}",
                source.display(),
                temporary.display()
            )
        })?;
    }
    if destination.exists() {
        fs::remove_file(destination).with_context(|| {
            format!(
                "could not replace stale Perry library {}",
                destination.display()
            )
        })?;
    }
    if let Err(error) = fs::rename(&temporary, destination) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| {
            format!(
                "could not publish shared library at Perry's expected path {}",
                destination.display()
            )
        });
    }
    Ok(())
}

pub struct CargoProfileProxy {
    directory: PathBuf,
    environment: Vec<(OsString, OsString)>,
}

impl CargoProfileProxy {
    pub fn new(project_root: &Path, engine_root: &Path, features: &[String]) -> Result<Self> {
        let cargo_environment = cargo_target_dir_environment_from(project_root)?;
        let real_cargo = executable_path_in_path("cargo").context(
            "Cargo was not found in PATH; install Rust to build this native BornEngine target",
        )?;
        let engine_root = engine_root
            .canonicalize()
            .context("could not resolve the installed BornEngine package")?;
        let current_executable =
            std::env::current_exe().context("could not locate the BornEngine CLI executable")?;
        let directory = create_proxy_directory(project_root)?;
        let executable_name = if cfg!(windows) { "cargo.exe" } else { "cargo" };
        let proxy_path = directory.join(executable_name);
        if let Err(error) = fs::copy(&current_executable, &proxy_path) {
            let _ = fs::remove_dir_all(&directory);
            return Err(error).with_context(|| {
                format!(
                    "could not prepare the Cargo profile wrapper at {}",
                    proxy_path.display()
                )
            });
        }
        if let Err(error) = make_executable(&proxy_path) {
            let _ = fs::remove_dir_all(&directory);
            return Err(error);
        }

        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut path_entries = vec![directory.clone()];
        path_entries.extend(std::env::split_paths(&old_path));
        let path = match std::env::join_paths(path_entries) {
            Ok(path) => path,
            Err(error) => {
                let _ = fs::remove_dir_all(&directory);
                return Err(error).context("could not extend PATH for Perry");
            }
        };
        let mut environment = vec![
            (OsString::from("PATH"), path),
            (
                OsString::from(PROXY_ENGINE_ROOT),
                engine_root.into_os_string(),
            ),
            (
                OsString::from(PROXY_REAL_CARGO),
                real_cargo.into_os_string(),
            ),
            (
                OsString::from(PROXY_FEATURES),
                OsString::from(features.join(",")),
            ),
        ];
        environment.extend(cargo_environment);
        Ok(Self {
            directory,
            environment,
        })
    }

    pub fn environment(&self) -> &[(OsString, OsString)] {
        &self.environment
    }
}

impl Drop for CargoProfileProxy {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn create_proxy_directory(project_root: &Path) -> Result<PathBuf> {
    let base = project_root.join(".bornengine/tmp");
    fs::create_dir_all(&base).with_context(|| {
        format!(
            "could not create CLI temporary directory {}",
            base.display()
        )
    })?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..8 {
        let directory = base.join(format!(
            "cargo-profile-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "could not create Cargo profile wrapper at {}",
                        directory.display()
                    )
                });
            }
        }
    }
    bail!("could not allocate a unique Cargo profile wrapper directory")
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).with_context(|| {
        format!(
            "could not make Cargo profile wrapper executable: {}",
            path.display()
        )
    })
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod cargo_cache_tests {
    use super::{cargo_target_dir_env, resolve_cargo_target_dir, resolve_cargo_target_dir_from};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    #[test]
    fn default_target_directory_uses_a_machine_wide_cache_root() {
        let cache_dir = Path::new("/user/cache");
        let resolved = resolve_cargo_target_dir(Some(cache_dir), None).unwrap();
        let expected = cache_dir.join("BornEngine").join("cargo-target");

        assert_eq!(resolved, expected);
    }

    #[test]
    fn caller_target_directory_is_preserved_verbatim() {
        let override_dir = OsString::from("../custom/../target-cache");
        let resolved = resolve_cargo_target_dir(None, Some(override_dir.clone())).unwrap();

        assert_eq!(resolved, PathBuf::from(&override_dir));
        assert!(
            cargo_target_dir_env(None, Some(override_dir))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn relative_target_directory_is_anchored_to_the_project_root() {
        let project_root = std::env::temp_dir().join("games").join("MyGame");
        let expected = project_root.join("../shared-cache");
        let target_dir = resolve_cargo_target_dir_from(
            None,
            Some(OsString::from("../shared-cache")),
            &project_root,
        )
        .unwrap();

        assert_eq!(target_dir, expected);
    }

    #[test]
    fn relative_target_directory_is_passed_as_absolute_to_subprocesses() {
        let project_root = std::env::temp_dir().join("games").join("MyGame");
        let expected = project_root.join("../shared-cache").into_os_string();
        let environment = super::cargo_target_dir_env_from(
            None,
            Some(OsString::from("../shared-cache")),
            &project_root,
        )
        .unwrap();

        assert_eq!(environment.len(), 1);
        assert_eq!(environment[0].0, "CARGO_TARGET_DIR");
        assert_eq!(environment[0].1, expected);
    }

    #[test]
    fn unavailable_user_cache_directory_has_an_actionable_error() {
        let error = resolve_cargo_target_dir(None, None).unwrap_err();

        assert!(error.to_string().contains("CARGO_TARGET_DIR"));
    }

    #[test]
    fn cargo_proxy_receives_the_default_target_directory_when_unset() {
        let cache_dir = Path::new("/user/cache");
        let expected = cache_dir
            .join("BornEngine")
            .join("cargo-target")
            .into_os_string();
        let env = cargo_target_dir_env(Some(cache_dir), None).unwrap();

        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "CARGO_TARGET_DIR");
        assert_eq!(env[0].1, expected);
    }

    #[test]
    fn native_development_profile_is_incremental_and_jobs_are_only_overridden_when_requested() {
        let development = super::native_build_environment(true, Some(3));
        assert!(development.contains(&("CARGO_PROFILE_DEV_OPT_LEVEL".into(), "1".into())));
        assert!(development.contains(&("CARGO_INCREMENTAL".into(), "1".into())));
        assert!(development.contains(&("CARGO_BUILD_JOBS".into(), "3".into())));

        let inherited_jobs = super::native_build_environment(true, None);
        assert!(
            !inherited_jobs
                .iter()
                .any(|(name, _)| name == "CARGO_BUILD_JOBS")
        );
        let release = super::native_build_environment(false, None);
        assert!(release.is_empty());
        assert_eq!(
            super::native_build_environment(false, Some(2)),
            vec![("CARGO_BUILD_JOBS".into(), "2".into())]
        );
    }
}

#[cfg(test)]
mod cargo_profile_artifact_tests {
    use super::mirror_engine_staticlib_for_perry;
    use std::ffi::OsString;
    use std::fs;
    use std::path::Path;

    #[test]
    fn shared_static_library_is_exposed_at_perrys_package_local_path() {
        let project = tempfile::tempdir().unwrap();
        let engine = project.path().join("node_modules/@bornengine/engine");
        let manifest = engine.join("native/linux/Cargo.toml");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            &manifest,
            "[package]\nname = \"bloom-linux\"\nversion = \"0.1.0\"\n\n[lib]\nname = \"bloom_linux\"\ncrate-type = [\"staticlib\"]\n",
        )
        .unwrap();
        let shared_target = project.path().join("shared-target");
        let archive = shared_target.join("release/libbloom_linux.a");
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        fs::write(&archive, b"native archive").unwrap();
        let args = [
            OsString::from("build"),
            OsString::from("--release"),
            OsString::from("--manifest-path"),
            manifest.into_os_string(),
        ];

        mirror_engine_staticlib_for_perry(&args, &engine, &shared_target).unwrap();

        let perry_archive = engine.join("native/linux/target/release/libbloom_linux.a");
        assert_eq!(fs::read(&perry_archive).unwrap(), b"native archive");
    }

    #[test]
    fn target_triple_and_non_release_profile_are_preserved_in_archive_path() {
        let project = tempfile::tempdir().unwrap();
        let engine = project.path().join("engine");
        let manifest = engine.join("native/windows/Cargo.toml");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            &manifest,
            "[package]\nname = \"bloom-windows\"\nversion = \"0.1.0\"\n\n[lib]\nname = \"bloom_windows\"\ncrate-type = [\"staticlib\"]\n",
        )
        .unwrap();
        let shared_target = project.path().join("shared-target");
        let archive = shared_target.join("x86_64-pc-windows-msvc/debug/bloom_windows.lib");
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        fs::write(&archive, b"windows archive").unwrap();
        let args = [
            OsString::from("build"),
            OsString::from("--target"),
            OsString::from("x86_64-pc-windows-msvc"),
            OsString::from("--manifest-path"),
            manifest.into_os_string(),
        ];

        mirror_engine_staticlib_for_perry(&args, &engine, &shared_target).unwrap();

        let perry_archive =
            engine.join("native/windows/target/x86_64-pc-windows-msvc/debug/bloom_windows.lib");
        assert_eq!(fs::read(perry_archive).unwrap(), b"windows archive");
    }

    #[test]
    fn unrelated_manifest_does_not_create_a_package_local_target_directory() {
        let project = tempfile::tempdir().unwrap();
        let engine = project.path().join("engine");
        fs::create_dir_all(&engine).unwrap();
        let manifest = project.path().join("game/Cargo.toml");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(&manifest, "[package]\nname = \"game\"\n").unwrap();
        let args = [
            OsString::from("build"),
            OsString::from("--manifest-path"),
            manifest.into_os_string(),
        ];

        mirror_engine_staticlib_for_perry(
            &args,
            &engine,
            Path::new("/does/not/matter/shared-target"),
        )
        .unwrap();

        assert!(!engine.join("native").exists());
    }
}
