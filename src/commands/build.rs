use crate::build_artifacts::{
    begin_build, begin_dev_build, clean_build_artifacts, record_build_files,
};
use crate::engine::engine_dependency;
use crate::platform::{
    BuildTarget, HostPlatform, PerryCapabilities, ResolvedTarget, TargetRequest, resolve_target,
};
use crate::process::{
    captured_command, ensure_success, inherited_command, perry_check_args, perry_compile_args,
    perry_dev_args,
};
use crate::project::{find_project_root, read_package_json};
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
    let output_name = derive_output_name(name, &context.package, &context.entry)?;
    let target_label = target_label(&context.target);
    let extension = context.target.target.output_extension();
    let artifact = begin_build(
        &context.project_root,
        &target_label,
        &output_name,
        extension,
    )?;
    let output = run_perry_compile(&context, &artifact.output, verbose)?;
    let recorded = record_build_files(&context.project_root, &artifact.directory)?;
    ensure_success("perry compile", &output)?;
    if recorded == 0 {
        bail!("Perry completed successfully without creating any build files");
    }
    if verbose {
        print_output(&output);
    }
    println!(
        "BornEngine\nTarget: {target_label}\nEntry: {}\nOutput: {}",
        entry_file.display(),
        artifact.output.display()
    );
    println!("Build completed successfully.");
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
    let output_name = derive_output_name(name, &context.package, &context.entry)?;
    let label = target_label(&context.target);
    let artifact = begin_build(
        &context.project_root,
        &label,
        &output_name,
        context.target.target.output_extension(),
    )?;
    let output = run_perry_compile(&context, &artifact.output, verbose)?;
    record_build_files(&context.project_root, &artifact.directory)?;
    ensure_success("perry compile", &output)?;
    if !artifact.output.is_file() {
        bail!(
            "Perry completed successfully but did not create {}",
            artifact.output.display()
        );
    }
    if verbose {
        print_output(&output);
    }
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
    let artifact = begin_dev_build(
        &context.project_root,
        &output_name,
        context.target.target.output_extension(),
    )?;
    let preexisting_objects = compiler_object_files(&context.project_root)?;
    let args = perry_dev_args(&context.entry, &artifact.output, verbose);
    let exit_code = inherited_command("perry", &args, Some(&context.project_root), verbose)?;
    move_new_compiler_objects(
        &context.project_root,
        &artifact.directory,
        &preexisting_objects,
    )?;
    record_build_files(&context.project_root, &artifact.directory)?;
    Ok(exit_code)
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
    println!("Perry check completed successfully.");
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
    Ok(BuildContext {
        project_root,
        entry,
        package,
        target,
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
    println!("Compiling...");
    let args = perry_compile_args(&context.entry, output_path, &context.target, verbose);
    let working_directory = output_path
        .parent()
        .context("build output path has no parent directory")?;
    captured_command("perry", &args, Some(working_directory), verbose)
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
