use crate::build_artifacts::{
    begin_build, begin_dev_build, clean_build_artifacts, record_build_files,
};
use crate::cargo_profile::{CargoProfileProxy, read_native_features, read_native_profile};
use crate::commands::assets::{pack_project, validate_project_assets};
use crate::engine::{EngineDependency, engine_dependency};
use crate::platform::{
    BuildTarget, HostPlatform, PerryCapabilities, ResolvedTarget, TargetRequest, resolve_target,
};
use crate::process::{
    captured_command, display_command, ensure_streamed_success, ensure_success, inherited_command,
    inherited_command_with_env, perry_check_args, perry_compile_args, perry_dev_args,
    streamed_command_with_env,
};
use crate::project::GameKind;
use crate::project::{find_project_root, read_package_json};
use crate::ui::{self, Tone};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct BuildContext {
    project_root: PathBuf,
    entry: PathBuf,
    package: Value,
    target: ResolvedTarget,
    native_profile: Option<GameKind>,
    engine_root: Option<PathBuf>,
}

pub fn derive_output_name(explicit: Option<&str>, package: &Value, entry: &Path) -> Result<String> {
    let name = if let Some(explicit) = explicit {
        explicit.to_owned()
    } else if let Some(name) = package.get("name").and_then(Value::as_str) {
        sanitize_output_name(name)
    } else {
        entry
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(sanitize_output_name)
            .unwrap_or_default()
    };
    validate_output_name(&name)?;
    Ok(name)
}

pub fn validate_run_target(target: &ResolvedTarget, host: HostPlatform) -> Result<()> {
    if !target.can_run_on(host) {
        let label = target
            .perry_target
            .as_deref()
            .unwrap_or_else(|| target.target.platform_name());
        bail!("target `{label}` cannot be run on the current `{host}` host");
    }
    Ok(())
}

pub fn build(
    entry_file: &Path,
    name: Option<&str>,
    os: Option<&str>,
    exact_target: Option<&str>,
    verbose: bool,
) -> Result<i32> {
    let context = resolve_context(entry_file, os, exact_target, verbose)?;
    validate_project_assets(&context.project_root)?;
    let output_name = derive_output_name(name, &context.package, &context.entry)?;
    let target_label = target_label(&context.target);
    let extension = context.target.target.output_extension();
    let artifact = begin_build(
        &context.project_root,
        &target_label,
        &output_name,
        extension,
    )?;
    let operation = (|| {
        let output = run_perry_compile(&context, &artifact.output, verbose)?;
        ensure_streamed_success("perry compile", &output)?;
        if !artifact.output.is_file() {
            bail!(
                "Perry completed successfully but did not create {}",
                artifact.output.display()
            );
        }
        pack_project(&context.project_root, &artifact.directory)?;
        Ok(output)
    })();
    let (_output, recorded) =
        finish_build_artifact(&context.project_root, &artifact.directory, operation)?;
    if recorded == 0 {
        bail!("Perry completed successfully without creating any build files");
    }
    println!("{}", ui::paint("BornEngine", Tone::Heading));
    println!(
        "Target: {target_label}\nEntry: {}\nOutput: {}",
        entry_file.display(),
        artifact.output.display()
    );
    println!(
        "{}",
        ui::paint("Build completed successfully.", Tone::Success)
    );
    Ok(0)
}

pub fn run(
    entry_file: &Path,
    name: Option<&str>,
    os: Option<&str>,
    exact_target: Option<&str>,
    program_args: &[String],
    verbose: bool,
) -> Result<i32> {
    let context = resolve_context(entry_file, os, exact_target, verbose)?;
    validate_run_target(&context.target, HostPlatform::current())?;
    validate_project_assets(&context.project_root)?;
    let output_name = derive_output_name(name, &context.package, &context.entry)?;
    let label = target_label(&context.target);
    let artifact = begin_build(
        &context.project_root,
        &label,
        &output_name,
        context.target.target.output_extension(),
    )?;
    let operation = (|| {
        let output = run_perry_compile(&context, &artifact.output, verbose)?;
        ensure_streamed_success("perry compile", &output)?;
        if !artifact.output.is_file() {
            bail!(
                "Perry completed successfully but did not create {}",
                artifact.output.display()
            );
        }
        pack_project(&context.project_root, &artifact.directory)?;
        Ok(output)
    })();
    let (_output, _) =
        finish_build_artifact(&context.project_root, &artifact.directory, operation)?;
    println!("{}", ui::paint("Launching game...", Tone::Info));
    let args = program_args.iter().map(OsString::from).collect::<Vec<_>>();
    inherited_command(
        artifact.output.to_string_lossy().as_ref(),
        &args,
        Some(&context.project_root),
        verbose,
    )
}

pub fn dev(
    entry_file: &Path,
    name: Option<&str>,
    os: Option<&str>,
    exact_target: Option<&str>,
    watch: bool,
    verbose: bool,
) -> Result<i32> {
    let context = resolve_context(entry_file, os, exact_target, verbose)?;
    validate_run_target(&context.target, HostPlatform::current())?;
    let output_name = derive_output_name(name, &context.package, &context.entry)?;
    if !watch {
        return run(
            entry_file,
            Some(&output_name),
            os,
            exact_target,
            &[],
            verbose,
        );
    }
    let assets = validate_project_assets(&context.project_root)?;
    let artifact = begin_dev_build(
        &context.project_root,
        &output_name,
        context.target.target.output_extension(),
    )?;
    let preexisting_objects = match compiler_object_files(&context.project_root) {
        Ok(objects) => objects,
        Err(error) => {
            let operation: Result<i32> = Err(error);
            let _ = finish_build_artifact(&context.project_root, &artifact.directory, operation)?;
            unreachable!("a failed operation cannot return a completed build artifact");
        }
    };
    let args = perry_dev_args(
        &context.entry,
        &artifact.output,
        &assets.watch_directories,
        verbose,
    );
    let profile_proxy = profile_proxy(&context, true)?;
    let environment = profile_proxy
        .as_ref()
        .map(CargoProfileProxy::environment)
        .unwrap_or_default();
    let command_result = inherited_command_with_env(
        "perry",
        &args,
        Some(&context.project_root),
        verbose,
        environment,
    );
    let move_result = move_new_compiler_objects(
        &context.project_root,
        &artifact.directory,
        &preexisting_objects,
    );
    let operation = combine_dev_results(command_result, move_result);
    let (exit_code, _) =
        finish_build_artifact(&context.project_root, &artifact.directory, operation)?;
    Ok(exit_code)
}

fn combine_dev_results(command_result: Result<i32>, move_result: Result<usize>) -> Result<i32> {
    match (command_result, move_result) {
        (Ok(exit_code), Ok(_)) => Ok(exit_code),
        (Err(command_error), Ok(_)) => Err(command_error),
        (Ok(_), Err(move_error)) => Err(move_error),
        (Err(command_error), Err(move_error)) => Err(command_error.context(format!(
            "also failed to move Perry intermediates into the managed build directory: {move_error:#}"
        ))),
    }
}

fn finish_build_artifact<T>(
    project_root: &Path,
    build_directory: &Path,
    operation: Result<T>,
) -> Result<(T, usize)> {
    let recording = record_build_files(project_root, build_directory);
    match (operation, recording) {
        (Ok(value), Ok(count)) => Ok((value, count)),
        (Err(operation_error), Ok(_)) => Err(operation_error),
        (Ok(_), Err(recording_error)) => Err(recording_error)
            .context("build operation succeeded but its outputs were not recorded"),
        (Err(operation_error), Err(recording_error)) => Err(operation_error.context(format!(
            "additionally failed to record partial build outputs: {recording_error:#}"
        ))),
    }
}

pub fn check(
    entry_file: &Path,
    os: Option<&str>,
    exact_target: Option<&str>,
    verbose: bool,
) -> Result<i32> {
    let context = resolve_context(entry_file, os, exact_target, verbose)?;
    let target = context.target.perry_target.as_deref();
    let args = perry_check_args(&context.entry, target, verbose);
    let output = captured_command("perry", &args, Some(&context.project_root), verbose)?;
    if verbose {
        print_output(&output);
    }
    ensure_success("perry check", &output)?;
    println!(
        "{}",
        ui::paint("Perry check completed successfully.", Tone::Success)
    );
    Ok(0)
}

pub fn clean(project_root: &Path) -> Result<usize> {
    clean_build_artifacts(project_root)
}

fn resolve_context(
    entry_file: &Path,
    os: Option<&str>,
    exact_target: Option<&str>,
    verbose: bool,
) -> Result<BuildContext> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let candidate = if entry_file.is_absolute() {
        entry_file.to_path_buf()
    } else {
        cwd.join(entry_file)
    };
    let entry = candidate
        .canonicalize()
        .with_context(|| format!("entry file does not exist: {}", entry_file.display()))?;
    if !entry.is_file() {
        bail!("entry path is not a file: {}", entry_file.display());
    }
    let project_root = find_project_root(&entry)?
        .or(find_project_root(&cwd)?)
        .context("could not find a BornEngine project; run this command inside a project")?;
    let package = read_package_json(&project_root)?;
    if engine_dependency(&package)?.is_none() {
        bail!(
            "{} does not declare a BornEngine dependency",
            project_root.join("package.json").display()
        );
    }
    let capabilities = perry_capabilities(verbose)?;
    let target = resolve_target(
        &TargetRequest {
            os: os.map(str::to_owned),
            target: exact_target.map(str::to_owned),
        },
        &capabilities,
        HostPlatform::current(),
    )?;
    let native_profile = native_profile(&project_root, &package)?;
    let engine_root = installed_engine_root(&project_root, &package)?;
    Ok(BuildContext {
        project_root,
        entry,
        package,
        target,
        native_profile,
        engine_root,
    })
}

fn perry_capabilities(verbose: bool) -> Result<PerryCapabilities> {
    let args = [OsString::from("compile"), OsString::from("--help")];
    let output = captured_command("perry", &args, None, verbose)?;
    ensure_success("perry compile --help", &output)?;
    let help = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(PerryCapabilities::from_compile_help(&help))
}

fn run_perry_compile(
    context: &BuildContext,
    output_path: &Path,
    verbose: bool,
) -> Result<std::process::Output> {
    let args = perry_compile_args(&context.entry, output_path, &context.target, verbose);
    let working_directory = output_path
        .parent()
        .context("build output path has no parent directory")?;
    if verbose {
        eprintln!(
            "{}",
            ui::paint_stderr(display_command("perry", &args), Tone::Accent)
        );
    }
    let profile_proxy = profile_proxy(context, false)?;
    let environment = profile_proxy
        .as_ref()
        .map(CargoProfileProxy::environment)
        .unwrap_or_default();
    ui::run_with_elapsed("Compiling", || {
        streamed_command_with_env("perry", &args, Some(working_directory), false, environment)
    })
}

fn profile_proxy(context: &BuildContext, development: bool) -> Result<Option<CargoProfileProxy>> {
    let (Some(_profile), Some(engine_root)) =
        (context.native_profile, context.engine_root.as_ref())
    else {
        return Ok(None);
    };
    if matches!(context.target.target, BuildTarget::Web | BuildTarget::Wasm) {
        return Ok(None);
    }
    let mut features = read_native_features(&context.project_root)?;
    if development && !features.iter().any(|feature| feature == "dev") {
        features.push("dev".to_owned());
    }
    Ok(Some(CargoProfileProxy::new(
        &context.project_root,
        engine_root,
        &features,
    )?))
}

fn native_profile(project_root: &Path, package: &Value) -> Result<Option<GameKind>> {
    let Some(dependency) = engine_dependency(package)? else {
        return Ok(None);
    };
    if dependency.package_name != "@bornengine/engine" {
        return Ok(None);
    }
    read_native_profile(project_root).map(Some)
}

fn installed_engine_root(project_root: &Path, package: &Value) -> Result<Option<PathBuf>> {
    let Some(EngineDependency { package_name, .. }) = engine_dependency(package)? else {
        return Ok(None);
    };
    if package_name != "@bornengine/engine" {
        return Ok(None);
    }
    let installed = project_root.join("node_modules").join(package_name);
    if !installed.exists() {
        // Keep TypeScript-only workflows and Perry test doubles usable when
        // dependencies have not yet been installed; Perry will report the
        // missing package when it resolves the actual game entry point.
        return Ok(None);
    }
    let root = installed.canonicalize().with_context(|| {
        format!(
            "could not find the installed BornEngine package at {}; install project dependencies first",
            installed.display()
        )
    })?;
    Ok(Some(root))
}

fn print_output(output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.trim().is_empty() {
        print!("{stdout}");
    }
    if !stderr.trim().is_empty() {
        eprint!("{stderr}");
    }
}

fn target_label(target: &ResolvedTarget) -> String {
    target
        .perry_target
        .clone()
        .unwrap_or_else(|| target.target.platform_name().to_owned())
}

fn compiler_object_files(project_root: &Path) -> Result<HashSet<OsString>> {
    let mut objects = HashSet::new();
    for entry in std::fs::read_dir(project_root)
        .with_context(|| format!("could not inspect {}", project_root.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_file() && is_perry_object_name(&entry.file_name()) {
            objects.insert(entry.file_name());
        }
    }
    Ok(objects)
}

fn move_new_compiler_objects(
    project_root: &Path,
    build_directory: &Path,
    preexisting: &HashSet<OsString>,
) -> Result<usize> {
    let mut moved = 0;
    for entry in std::fs::read_dir(project_root)
        .with_context(|| format!("could not inspect {}", project_root.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        if preexisting.contains(&name)
            || !entry.file_type()?.is_file()
            || !is_perry_object_name(&name)
        {
            continue;
        }
        let destination = build_directory.join(&name);
        std::fs::rename(entry.path(), &destination).with_context(|| {
            format!(
                "could not move Perry intermediate {} into the managed build directory",
                entry.path().display()
            )
        })?;
        moved += 1;
    }
    Ok(moved)
}

fn is_perry_object_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(stem) = name.strip_suffix(".o") else {
        return false;
    };
    ["_ts", "_tsx", "_mts", "_cts"]
        .iter()
        .any(|suffix| stem.ends_with(suffix))
}

fn validate_output_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        bail!("output name must be a simple file name without path separators");
    }
    Ok(())
}

fn sanitize_output_name(name: &str) -> String {
    let mut normalized = String::new();
    let mut previous_dash = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
            normalized.push(character.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash && !normalized.is_empty() {
            normalized.push('-');
            previous_dash = true;
        }
    }
    normalized.trim_matches(['-', '.']).to_owned()
}

impl BuildTarget {
    fn platform_name(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::MacOS => "macos",
            Self::Android => "android",
            Self::IOS => "ios",
            Self::IosSimulator => "ios-simulator",
            Self::TvOS => "tvos",
            Self::TvOSSimulator => "tvos-simulator",
            Self::WatchOS => "watchos",
            Self::WatchOSSimulator => "watchos-simulator",
            Self::VisionOS => "visionos",
            Self::VisionOSSimulator => "visionos-simulator",
            Self::WearOS => "wearos",
            Self::Web => "web",
            Self::Wasm => "wasm",
            Self::Other => "custom",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_perry_object_name;
    use std::ffi::OsStr;

    #[test]
    fn identifies_only_perry_compiled_typescript_object_names() {
        assert!(is_perry_object_name(OsStr::new("main_ts.o")));
        assert!(is_perry_object_name(OsStr::new("module_tsx.o")));
        assert!(!is_perry_object_name(OsStr::new("custom.o")));
        assert!(!is_perry_object_name(OsStr::new("main.ts")));
    }
}
