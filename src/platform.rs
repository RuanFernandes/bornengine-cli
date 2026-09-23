use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostPlatform {
    Linux,
    Windows,
    MacOS,
    Other,
}

impl HostPlatform {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "linux" => Self::Linux,
            "windows" => Self::Windows,
            "macos" => Self::MacOS,
            _ => Self::Other,
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "linux" => Self::Linux,
            "windows" => Self::Windows,
            "macos" | "darwin" => Self::MacOS,
            _ => Self::Other,
        }
    }
}

impl fmt::Display for HostPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::MacOS => "macos",
            Self::Other => "unknown",
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PerryCapabilities {
    supported_targets: BTreeSet<String>,
}

impl PerryCapabilities {
    pub fn from_compile_help(help: &str) -> Self {
        let mut supported_targets = BTreeSet::new();
        for line in help.lines() {
            let Some((_, targets)) = line.split_once("Target platform:") else {
                continue;
            };
            let targets = targets.split("(default:").next().unwrap_or(targets);
            for target in targets.split(',') {
                let target = target
                    .trim()
                    .split_ascii_whitespace()
                    .next()
                    .unwrap_or_default()
                    .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
                if !target.is_empty() {
                    supported_targets.insert(target.to_owned());
                }
            }
        }
        Self { supported_targets }
    }

    pub fn supports(&self, target: &str) -> bool {
        self.supported_targets.contains(target)
    }

    pub fn supported_targets(&self) -> impl Iterator<Item = &str> {
        self.supported_targets.iter().map(String::as_str)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TargetRequest {
    pub os: Option<String>,
    pub target: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildTarget {
    Linux,
    Windows,
    MacOS,
    Android,
    IOS,
    IosSimulator,
    TvOS,
    TvOSSimulator,
    WatchOS,
    WatchOSSimulator,
    VisionOS,
    VisionOSSimulator,
    WearOS,
    Web,
    Wasm,
}

impl BuildTarget {
    pub fn from_perry_target(value: &str) -> Result<Self> {
        match value {
            "linux" => Ok(Self::Linux),
            "windows" => Ok(Self::Windows),
            "macos" => Ok(Self::MacOS),
            "android" => Ok(Self::Android),
            "ios" => Ok(Self::IOS),
            "ios-simulator" => Ok(Self::IosSimulator),
            "tvos" => Ok(Self::TvOS),
            "tvos-simulator" => Ok(Self::TvOSSimulator),
            "watchos" => Ok(Self::WatchOS),
            "watchos-simulator" => Ok(Self::WatchOSSimulator),
            "visionos" => Ok(Self::VisionOS),
            "visionos-simulator" => Ok(Self::VisionOSSimulator),
            "wearos" => Ok(Self::WearOS),
            "web" => Ok(Self::Web),
            "wasm" => Ok(Self::Wasm),
            _ => bail!("unsupported Perry target `{value}`"),
        }
    }

    pub fn perry_target(self) -> Option<&'static str> {
        match self {
            Self::Linux => Some("linux"),
            Self::Windows => Some("windows"),
            Self::MacOS => None,
            Self::Android => Some("android"),
            Self::IOS => Some("ios"),
            Self::IosSimulator => Some("ios-simulator"),
            Self::TvOS => Some("tvos"),
            Self::TvOSSimulator => Some("tvos-simulator"),
            Self::WatchOS => Some("watchos"),
            Self::WatchOSSimulator => Some("watchos-simulator"),
            Self::VisionOS => Some("visionos"),
            Self::VisionOSSimulator => Some("visionos-simulator"),
            Self::WearOS => Some("wearos"),
            Self::Web => Some("web"),
            Self::Wasm => Some("wasm"),
        }
    }

    pub fn output_extension(self) -> Option<&'static str> {
        match self {
            Self::Windows => Some("exe"),
            Self::Web | Self::Wasm => Some("html"),
            _ => None,
        }
    }

    pub fn can_run_on(self, host: HostPlatform) -> bool {
        matches!(
            (self, host),
            (Self::Linux, HostPlatform::Linux)
                | (Self::Windows, HostPlatform::Windows)
                | (Self::MacOS, HostPlatform::MacOS)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedTarget {
    pub target: BuildTarget,
    pub perry_target: Option<String>,
}

impl ResolvedTarget {
    pub fn can_run_on(&self, host: HostPlatform) -> bool {
        self.target.can_run_on(host)
    }
}

pub fn resolve_target(
    request: &TargetRequest,
    capabilities: &PerryCapabilities,
    host: HostPlatform,
) -> Result<ResolvedTarget> {
    if request.os.is_some() && request.target.is_some() {
        bail!("choose either `--os` or `--target`, not both");
    }

    if let Some(exact) = request.target.as_deref() {
        return resolve_perry_target(exact, capabilities);
    }

    let friendly_os = request.os.as_deref().map(str::to_ascii_lowercase);
    let platform = match friendly_os.as_deref() {
        Some("linux") => BuildTarget::Linux,
        Some("windows") => BuildTarget::Windows,
        Some("macos" | "darwin") => {
            if host != HostPlatform::MacOS {
                bail!(
                    "macOS builds require a macOS host because Perry uses the native host target"
                );
            }
            return Ok(ResolvedTarget {
                target: BuildTarget::MacOS,
                perry_target: None,
            });
        }
        Some("android") => BuildTarget::Android,
        Some("ios") => BuildTarget::IOS,
        Some("tvos") => BuildTarget::TvOS,
        Some("watchos") => BuildTarget::WatchOS,
        Some("visionos") => BuildTarget::VisionOS,
        Some("web") => BuildTarget::Web,
        Some(other) => bail!("unsupported operating system `{other}`"),
        None => match host {
            HostPlatform::Linux => BuildTarget::Linux,
            HostPlatform::Windows => BuildTarget::Windows,
            HostPlatform::MacOS => {
                return Ok(ResolvedTarget {
                    target: BuildTarget::MacOS,
                    perry_target: None,
                });
            }
            HostPlatform::Other => bail!("the current host platform is not supported"),
        },
    };

    let perry_target = platform
        .perry_target()
        .expect("all non-macOS targets have Perry target names");
    ensure_target_supported(perry_target, capabilities)?;
    Ok(ResolvedTarget {
        target: platform,
        perry_target: Some(perry_target.to_owned()),
    })
}

fn resolve_perry_target(value: &str, capabilities: &PerryCapabilities) -> Result<ResolvedTarget> {
    let target = BuildTarget::from_perry_target(value)?;
    ensure_target_supported(value, capabilities)?;
    Ok(ResolvedTarget {
        target,
        perry_target: Some(value.to_owned()),
    })
}

fn ensure_target_supported(value: &str, capabilities: &PerryCapabilities) -> Result<()> {
    if !capabilities.supports(value) {
        let available = capabilities
            .supported_targets()
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "installed Perry does not advertise target `{value}`; supported targets: {available}"
        );
    }
    Ok(())
}
