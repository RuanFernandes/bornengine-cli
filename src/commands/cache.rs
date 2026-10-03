use crate::cargo_profile::{
    args_with_profile, cargo_target_dir_environment, effective_cargo_target_dir,
    native_build_environment, read_native_features, read_native_profile,
};
use crate::cli::CacheCommands;
use crate::engine::{EngineDependency, engine_dependency};
use crate::platform::HostPlatform;
use crate::process::{ensure_streamed_success, streamed_command_with_env};
use crate::project::{find_project_root, read_package_json};
use crate::ui::{self, Tone};
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn execute(command: CacheCommands, verbose: bool) -> Result<i32> {
    match command {
        CacheCommands::Path => {
            println!("{}", effective_cargo_target_dir()?.display());
            Ok(0)
        }
        CacheCommands::Warm { release, jobs } => warm(release, jobs, verbose),
    }
}

pub fn native_manifest_path(engine_root: &Path, host: HostPlatform) -> Result<PathBuf> {
    let platform = match host {
        HostPlatform::Linux => "linux",
        HostPlatform::Windows => "windows",
        HostPlatform::MacOS => "macos",
        HostPlatform::Other => bail!(
            "`bornengine cache warm` requires a Linux, Windows, or macOS host; Web/WASM uses its prebuilt engine artifact"
        ),
    };
    let manifest = engine_root.join("native").join(platform).join("Cargo.toml");
    if !manifest.is_file() {
        bail!(
            "the installed BornEngine package does not contain the native {platform} manifest at {}; reinstall project dependencies or use the native target for this host",
            manifest.display()
        );
    }
    Ok(manifest)
}

fn warm(release: bool, jobs: Option<usize>, verbose: bool) -> Result<i32> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let project_root = find_project_root(&cwd)?
        .context("could not find a BornEngine project; run `cache warm` inside a game project")?;
    let package = read_package_json(&project_root)?;
    let dependency = engine_dependency(&package)?
        .context("this project does not declare a BornEngine dependency")?;
    if dependency.package_name != "@bornengine/engine" {
        bail!(
            "`bornengine cache warm` currently supports native builds of `@bornengine/engine`; this project uses `{}`",
            dependency.package_name
        );
    }
    let engine_root = installed_engine_root(&project_root, &dependency)?;
    let manifest = native_manifest_path(&engine_root, HostPlatform::current())?;
    let native_profile = read_native_profile(&project_root)?;
    let features = read_native_features(&project_root)?;
    let feature_refs = features.iter().map(String::as_str).collect::<Vec<_>>();
    let base_args = vec![
        OsString::from("build"),
        OsString::from("--manifest-path"),
        manifest.into_os_string(),
    ];
    let mut args = args_with_profile(&base_args, &engine_root, &feature_refs);
    if release {
        args.push(OsString::from("--release"));
    }

    let mut environment = cargo_target_dir_environment()?;
    environment.extend(native_build_environment(!release, jobs));
    println!(
        "Warming BornEngine {} ({}) in {}",
        dependency.spec,
        native_profile.label(),
        effective_cargo_target_dir()?.display()
    );
    let output = ui::run_with_elapsed("Warming native engine cache", || {
        streamed_command_with_env("cargo", &args, Some(&project_root), verbose, &environment)
    })?;
    ensure_streamed_success("cargo build", &output)?;
    println!("{}", ui::paint("Native cache is ready.", Tone::Success));
    Ok(0)
}

fn installed_engine_root(project_root: &Path, dependency: &EngineDependency) -> Result<PathBuf> {
    let installed = project_root
        .join("node_modules")
        .join(&dependency.package_name);
    if !installed.exists() {
        bail!(
            "installed BornEngine package was not found at {}; install project dependencies first",
            installed.display()
        );
    }
    installed.canonicalize().with_context(|| {
        format!(
            "could not resolve the installed BornEngine package at {}",
            installed.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::native_manifest_path;
    use crate::platform::HostPlatform;
    use std::fs;

    #[test]
    fn native_manifest_resolution_covers_supported_hosts() {
        let root = tempfile::tempdir().unwrap();
        for (host, platform) in [
            (HostPlatform::Linux, "linux"),
            (HostPlatform::Windows, "windows"),
            (HostPlatform::MacOS, "macos"),
        ] {
            let manifest = root.path().join("native").join(platform).join("Cargo.toml");
            fs::create_dir_all(manifest.parent().unwrap()).unwrap();
            fs::write(&manifest, "[package]\nname = \"fixture\"\n").unwrap();

            assert_eq!(native_manifest_path(root.path(), host).unwrap(), manifest);
        }
    }

    #[test]
    fn native_manifest_resolution_explains_unsupported_hosts() {
        let root = tempfile::tempdir().unwrap();
        let error = native_manifest_path(root.path(), HostPlatform::Other).unwrap_err();

        assert!(error.to_string().contains("Web/WASM"));
    }
}
