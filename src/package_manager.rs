use anyhow::{Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
#[value(rename_all = "lowercase")]
pub enum PackageManager {
    Pnpm,
    Npm,
    Yarn,
}

impl PackageManager {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "pnpm" => Ok(Self::Pnpm),
            "npm" => Ok(Self::Npm),
            "yarn" => Ok(Self::Yarn),
            _ => bail!("unsupported package manager `{value}`"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pnpm => "pnpm",
            Self::Npm => "npm",
            Self::Yarn => "yarn",
        }
    }

    pub fn executable(self) -> &'static str {
        self.as_str()
    }

    pub fn install_args(self) -> &'static [&'static str] {
        match self {
            Self::Pnpm | Self::Npm | Self::Yarn => &["install"],
        }
    }
}
