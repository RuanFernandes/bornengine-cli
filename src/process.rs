use crate::platform::ResolvedTarget;
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output, Stdio};

pub fn perry_compile_args(
    entry: &Path,
    output: &Path,
    target: &ResolvedTarget,
    verbose: bool,
) -> Vec<OsString> {
    let mut args = vec![
        "compile".into(),
        entry.as_os_str().to_owned(),
        "--output".into(),
        output.as_os_str().to_owned(),
    ];
    if let Some(target) = target.perry_target.as_deref() {
        args.extend(["--target".into(), target.into()]);
    }
    if verbose {
        args.push("-v".into());
    }
    args
}

pub fn perry_check_args(entry: &Path, target: Option<&str>, verbose: bool) -> Vec<OsString> {
    let mut args = vec!["check".into(), entry.as_os_str().to_owned()];
    if let Some(target) = target {
        args.extend(["--target".into(), target.into()]);
    }
    if verbose {
        args.push("-v".into());
    }
    args
}

pub fn perry_dev_args(entry: &Path, output: &Path, verbose: bool) -> Vec<OsString> {
    let mut args = vec![
        "dev".into(),
        entry.as_os_str().to_owned(),
        "--output".into(),
        output.as_os_str().to_owned(),
    ];
    if verbose {
        args.push("-v".into());
    }
    args
}

pub fn captured_command(
    program: &str,
    args: &[OsString],
    cwd: Option<&Path>,
    verbose: bool,
) -> Result<Output> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    if verbose {
        eprintln!("{}", display_command(program, args));
    }
    command.output().with_context(|| {
        format!("could not start `{program}`; make sure it is installed and on PATH")
    })
}

pub fn inherited_command(
    program: &str,
    args: &[OsString],
    cwd: Option<&Path>,
    verbose: bool,
) -> Result<i32> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    if verbose {
        eprintln!("{}", display_command(program, args));
    }
    let status = command.status().with_context(|| {
        format!("could not start `{program}`; make sure it is installed and on PATH")
    })?;
    Ok(status.code().unwrap_or(1))
}

pub fn ensure_success(program: &str, output: &Output) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let details = if stderr.is_empty() { stdout } else { stderr };
    if details.is_empty() {
        bail!("`{program}` exited with status {}", output.status);
    }
    bail!("`{program}` failed: {details}");
}

pub fn display_command(program: &str, args: &[OsString]) -> String {
    std::iter::once(program.to_owned())
        .chain(args.iter().map(|arg| arg.to_string_lossy().into_owned()))
        .map(|arg| {
            if arg.chars().any(char::is_whitespace) {
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                arg
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn executable_in_path(executable: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        if cfg!(windows) {
            let candidate = directory.join(executable);
            if candidate.is_file() {
                return true;
            }
            let extensions =
                std::env::var_os("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
            extensions
                .to_string_lossy()
                .split(';')
                .any(|extension| directory.join(format!("{executable}{extension}")).is_file())
        } else {
            directory.join(executable).is_file()
        }
    })
}
