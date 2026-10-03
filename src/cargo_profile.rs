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

pub fn effective_cargo_target_dir() -> Result<PathBuf> {
    let cache_dir = BaseDirs::new().map(|directories| directories.cache_dir().to_path_buf());
    resolve_cargo_target_dir(cache_dir.as_deref(), std::env::var_os(CARGO_TARGET_DIR))
}

pub fn cargo_target_dir_environment() -> Result<Vec<(OsString, OsString)>> {
    let override_dir = std::env::var_os(CARGO_TARGET_DIR);
    if override_dir.is_some() {
        return Ok(Vec::new());
    }
    let cache_dir = BaseDirs::new().map(|directories| directories.cache_dir().to_path_buf());
    cargo_target_dir_env(cache_dir.as_deref(), None)
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
    match Command::new(real_cargo).args(args).status() {
        Ok(status) => Some(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("could not start Cargo for the BornEngine build: {error}");
            Some(1)
        }
    }
}

pub struct CargoProfileProxy {
    directory: PathBuf,
    environment: Vec<(OsString, OsString)>,
}

impl CargoProfileProxy {
    pub fn new(project_root: &Path, engine_root: &Path, features: &[String]) -> Result<Self> {
        let cargo_environment = cargo_target_dir_environment()?;
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
    use super::{cargo_target_dir_env, resolve_cargo_target_dir};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    #[test]
    fn default_target_directory_uses_a_machine_wide_cache_root() {
        let cache_dir = Path::new("/user/cache");
        let resolved = resolve_cargo_target_dir(Some(cache_dir), None).unwrap();

        assert_eq!(
            resolved,
            PathBuf::from("/user/cache/BornEngine/cargo-target")
        );
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
    fn unavailable_user_cache_directory_has_an_actionable_error() {
        let error = resolve_cargo_target_dir(None, None).unwrap_err();

        assert!(error.to_string().contains("CARGO_TARGET_DIR"));
    }

    #[test]
    fn cargo_proxy_receives_the_default_target_directory_when_unset() {
        let env = cargo_target_dir_env(Some(Path::new("/user/cache")), None).unwrap();

        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "CARGO_TARGET_DIR");
        assert_eq!(env[0].1, "/user/cache/BornEngine/cargo-target");
    }

    #[test]
    fn native_development_profile_is_incremental_and_release_is_untouched() {
        let development = super::native_build_environment(true, None);
        assert!(development.contains(&("CARGO_PROFILE_DEV_OPT_LEVEL".into(), "1".into())));
        assert!(development.contains(&("CARGO_INCREMENTAL".into(), "1".into())));
        assert!(super::native_build_environment(false, None).is_empty());
    }
}
