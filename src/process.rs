use crate::platform::ResolvedTarget;
use anyhow::{Context, Result, bail};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
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
    let mut command = Command::new(program_for_spawn(program));
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
    let mut command = Command::new(program_for_spawn(program));
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
    find_program_in_path(OsStr::new(executable), &path, &path_extensions()).is_some()
}

fn program_for_spawn(program: &str) -> OsString {
    #[cfg(windows)]
    {
        let Some(path) = std::env::var_os("PATH") else {
            return OsString::from(program);
        };
        return resolve_command_program(OsStr::new(program), &path, &path_extensions());
    }

    #[cfg(not(windows))]
    OsString::from(program)
}

#[cfg(any(windows, test))]
fn resolve_command_program(
    program: &OsStr,
    search_path: &OsStr,
    extensions: &[OsString],
) -> OsString {
    let path = Path::new(program);
    if path.components().count() != 1 || path.extension().is_some() {
        return program.to_os_string();
    }

    find_program_in_path(program, search_path, extensions)
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|| program.to_os_string())
}

fn find_program_in_path(
    program: &OsStr,
    search_path: &OsStr,
    extensions: &[OsString],
) -> Option<PathBuf> {
    let has_explicit_extension = Path::new(program).extension().is_some();

    for directory in std::env::split_paths(search_path) {
        let candidate = directory.join(program);
        if (has_explicit_extension || extensions.is_empty()) && candidate.is_file() {
            return Some(make_path_absolute(candidate));
        }

        for extension in extensions {
            let mut filename = program.to_os_string();
            filename.push(extension);
            let candidate = directory.join(filename);
            if candidate.is_file() {
                return Some(make_path_absolute(candidate));
            }
        }
    }

    None
}

fn make_path_absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map(|current_dir| current_dir.join(&path))
            .unwrap_or(path)
    }
}

fn path_extensions() -> Vec<OsString> {
    #[cfg(windows)]
    {
        let configured =
            std::env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
        let supported = [".COM", ".EXE", ".BAT", ".CMD"];
        let mut extensions = configured
            .to_string_lossy()
            .split(';')
            .map(str::trim)
            .filter(|extension| {
                supported
                    .iter()
                    .any(|supported| extension.eq_ignore_ascii_case(supported))
            })
            .map(OsString::from)
            .collect::<Vec<_>>();

        for extension in supported {
            if !extensions
                .iter()
                .any(|existing| existing.to_string_lossy().eq_ignore_ascii_case(extension))
            {
                extensions.push(OsString::from(extension));
            }
        }

        extensions
    }

    #[cfg(not(windows))]
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::resolve_command_program;
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn path_value(directory: &Path) -> OsString {
        env::join_paths([directory]).unwrap()
    }

    #[test]
    fn resolves_windows_command_shim_to_its_pathext_path() {
        let directory = tempfile::tempdir().unwrap();
        let npm_cmd = directory.path().join("npm.cmd");
        fs::write(&npm_cmd, "@echo off\r\n").unwrap();
        let path = path_value(directory.path());
        let extensions = [".com", ".exe", ".bat", ".cmd"].map(OsString::from);

        let resolved = resolve_command_program(OsStr::new("npm"), &path, &extensions);

        assert_eq!(PathBuf::from(resolved), npm_cmd);
    }

    #[test]
    fn prefers_windows_command_shim_over_extensionless_unix_launcher() {
        let directory = tempfile::tempdir().unwrap();
        let unix_launcher = directory.path().join("npm");
        let windows_launcher = directory.path().join("npm.cmd");
        fs::write(&unix_launcher, "#!/bin/sh\necho npm\n").unwrap();
        fs::write(&windows_launcher, "@echo off\r\n").unwrap();
        let path = path_value(directory.path());
        let extensions = [".com", ".exe", ".bat", ".cmd"].map(OsString::from);

        let resolved = resolve_command_program(OsStr::new("npm"), &path, &extensions);

        assert_eq!(PathBuf::from(resolved), windows_launcher);
    }

    #[test]
    fn does_not_resolve_extensionless_unix_launcher_as_windows_command() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("npm"), "#!/bin/sh\necho npm\n").unwrap();
        let path = path_value(directory.path());
        let extensions = [".com", ".exe", ".bat", ".cmd"].map(OsString::from);

        let resolved = resolve_command_program(OsStr::new("npm"), &path, &extensions);

        assert_eq!(resolved, OsString::from("npm"));
    }

    #[cfg(windows)]
    #[test]
    fn launches_the_selected_windows_command_shim() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("npm"), "#!/bin/sh\necho wrong shim\n").unwrap();
        let windows_launcher = directory.path().join("npm.cmd");
        fs::write(&windows_launcher, "@echo off\r\necho windows shim\r\n").unwrap();
        let path = path_value(directory.path());
        let extensions = [".com", ".exe", ".bat", ".cmd"].map(OsString::from);
        let resolved = resolve_command_program(OsStr::new("npm"), &path, &extensions);

        let output = std::process::Command::new(resolved).output().unwrap();

        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "windows shim"
        );
    }

    #[test]
    fn command_resolution_preserves_pathext_order_when_multiple_shims_exist() {
        let directory = tempfile::tempdir().unwrap();
        let npm_exe = directory.path().join("npm.exe");
        let npm_cmd = directory.path().join("npm.cmd");
        fs::write(&npm_exe, "").unwrap();
        fs::write(&npm_cmd, "").unwrap();
        let path = path_value(directory.path());
        let extensions = [".cmd", ".exe"].map(OsString::from);

        let resolved = resolve_command_program(OsStr::new("npm"), &path, &extensions);

        assert_eq!(PathBuf::from(resolved), npm_cmd);
    }
}
