use crate::package_manager::PackageManager;
use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct Config {
    pub package_manager: String,
    pub engine_version: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            package_manager: PackageManager::Pnpm.as_str().to_owned(),
            engine_version: "latest".to_owned(),
        }
    }
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let base_dirs = BaseDirs::new().context("could not determine the user config directory")?;
        Ok(base_dirs
            .config_dir()
            .join("bornengine")
            .join("config.toml"))
    }

    pub fn load_default() -> Result<Self> {
        Self::load(&Self::path()?)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("could not read {}", path.display()));
            }
        };
        let config: Self = toml::from_str(&contents)
            .with_context(|| format!("invalid BornEngine configuration in {}", path.display()))?;
        PackageManager::parse(&config.package_manager)
            .with_context(|| format!("invalid package_manager value in {}", path.display()))?;
        validate_engine_version(&config.engine_version)?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        PackageManager::parse(&self.package_manager)?;
        validate_engine_version(&self.engine_version)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("could not create config directory {}", parent.display())
            })?;
        }
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                bail!(
                    "refusing to update non-regular config file {}",
                    path.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("could not inspect {}", path.display()));
            }
        }
        let contents = toml::to_string_pretty(self).context("could not serialize configuration")?;
        fs::write(path, contents).with_context(|| format!("could not write {}", path.display()))
    }

    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "package-manager" | "package_manager" => Some(self.package_manager.clone()),
            "engine-version" | "engine_version" => Some(self.engine_version.clone()),
            _ => None,
        }
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "package-manager" | "package_manager" => {
                self.package_manager = PackageManager::parse(value)?.as_str().to_owned();
            }
            "engine-version" | "engine_version" => {
                validate_engine_version(value)?;
                self.engine_version = value.to_owned();
            }
            _ => bail!("unsupported configuration key `{key}`"),
        }
        Ok(())
    }

    pub fn entries(&self) -> [(&'static str, &str); 2] {
        [
            ("package-manager", &self.package_manager),
            ("engine-version", &self.engine_version),
        ]
    }
}

fn validate_engine_version(value: &str) -> Result<()> {
    if value == "latest" {
        return Ok(());
    }
    let version = semver::Version::parse(value).with_context(|| {
        format!("engine version must be `latest` or an exact semantic version, got `{value}`")
    })?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        bail!("engine version must be a stable release without prerelease or build metadata");
    }
    Ok(())
}
