use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const VALID_MANIFEST: &str = r#"{
  "format": "bornengine-script-v1",
  "apiVersion": 1,
  "entry": "scripts/actor.js",
  "permissions": ["log", "self.read"]
}"#;

fn write_package(root: &Path, manifest: &str, entry: Option<(&str, &[u8])>) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    let manifest_path = root.join("bornengine.script.json");
    fs::write(&manifest_path, manifest).unwrap();
    if let Some((path, contents)) = entry {
        let entry_path = root.join(path);
        fs::create_dir_all(entry_path.parent().unwrap()).unwrap();
        fs::write(entry_path, contents).unwrap();
    }
    manifest_path
}

fn run_check(manifest: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args(["script", "check", "--manifest"])
        .arg(manifest)
        .output()
        .unwrap()
}

fn run_pack(manifest: &Path, output: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args(["script", "pack", "--output"])
        .arg(output)
        .args(["--manifest"])
        .arg(manifest)
        .output()
        .unwrap()
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn collect_files(root: &Path, relative: &Path, files: &mut Vec<(String, Vec<u8>)>) {
    let directory = root.join(relative);
    let mut entries = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    for entry in entries {
        let child = entry.strip_prefix(root).unwrap();
        if entry.is_dir() {
            collect_files(root, child, files);
        } else {
            files.push((
                child.to_string_lossy().replace('\\', "/"),
                fs::read(entry).unwrap(),
            ));
        }
    }
}

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    collect_files(root, Path::new("."), &mut files);
    files
}

#[test]
fn script_check_accepts_valid_package() {
    let package = tempfile::tempdir().unwrap();
    let manifest = write_package(
        package.path(),
        VALID_MANIFEST,
        Some(("scripts/actor.js", b"export default { update() {} };")),
    );

    let result = run_check(&manifest);
    assert!(result.status.success(), "{}", output_text(&result));
    assert!(
        output_text(&result).contains("valid"),
        "{}",
        output_text(&result)
    );
}

#[test]
fn script_check_rejects_unknown_api_or_permission() {
    let package = tempfile::tempdir().unwrap();
    let unknown_api = write_package(
        package.path(),
        &VALID_MANIFEST.replace("\"apiVersion\": 1", "\"apiVersion\": 2"),
        Some(("scripts/actor.js", b"export default {}")),
    );
    let result = run_check(&unknown_api);
    assert!(!result.status.success(), "unknown API version was accepted");
    assert!(
        output_text(&result).to_lowercase().contains("api"),
        "{}",
        output_text(&result)
    );

    let unknown_permission = package.path().join("unknown-permission.json");
    fs::write(
        &unknown_permission,
        VALID_MANIFEST.replace("\"log\", \"self.read\"", "\"log\", \"network\""),
    )
    .unwrap();
    let result = run_check(&unknown_permission);
    assert!(!result.status.success(), "unknown permission was accepted");
    assert!(
        output_text(&result).to_lowercase().contains("permission"),
        "{}",
        output_text(&result)
    );
}

#[test]
fn script_check_rejects_missing_and_escaping_entry() {
    let package = tempfile::tempdir().unwrap();
    let missing = write_package(
        &package.path().join("missing"),
        &VALID_MANIFEST.replace("scripts/actor.js", "scripts/missing.js"),
        None,
    );
    let result = run_check(&missing);
    assert!(!result.status.success(), "missing entry was accepted");
    assert!(
        output_text(&result).to_lowercase().contains("entry"),
        "{}",
        output_text(&result)
    );

    fs::write(package.path().join("outside.js"), b"export default {};").unwrap();
    let escaping = write_package(
        &package.path().join("escaping"),
        &VALID_MANIFEST.replace("scripts/actor.js", "../outside.js"),
        None,
    );
    let result = run_check(&escaping);
    assert!(!result.status.success(), "escaping entry was accepted");
    let diagnostic = output_text(&result).to_lowercase();
    assert!(
        diagnostic.contains("unsafe") || diagnostic.contains("escape"),
        "{diagnostic}"
    );
}

#[test]
fn script_pack_contains_only_the_declared_entry() {
    let package = tempfile::tempdir().unwrap();
    let manifest = write_package(
        package.path(),
        VALID_MANIFEST,
        Some(("scripts/actor.js", b"export default { update() {} };")),
    );
    fs::write(package.path().join("scripts/ignored.js"), b"do not pack").unwrap();
    let output = package.path().join("dist");

    let result = run_pack(&manifest, &output);
    assert!(result.status.success(), "{}", output_text(&result));
    assert_eq!(
        snapshot(&output)
            .iter()
            .map(|item| item.0.as_str())
            .collect::<Vec<_>>(),
        ["bornengine.script.json", "scripts/actor.js"]
    );
    assert_eq!(
        fs::read(output.join("scripts/actor.js")).unwrap(),
        b"export default { update() {} };"
    );
}

#[test]
fn script_pack_is_deterministic() {
    let package = tempfile::tempdir().unwrap();
    let manifest = write_package(
        package.path(),
        VALID_MANIFEST,
        Some(("scripts/actor.js", b"export default { update() {} };")),
    );
    let output = package.path().join("dist");

    let first = run_pack(&manifest, &output);
    assert!(first.status.success(), "{}", output_text(&first));
    let first_snapshot = snapshot(&output);
    let second = run_pack(&manifest, &output);
    assert!(second.status.success(), "{}", output_text(&second));
    assert_eq!(snapshot(&output), first_snapshot);
}
