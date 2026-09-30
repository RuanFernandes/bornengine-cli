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
    assert!(
        output_text(&result).contains("Packed 3 script package files"),
        "{}",
        output_text(&result)
    );
    assert_eq!(
        snapshot(&output)
            .iter()
            .map(|item| item.0.as_str())
            .collect::<Vec<_>>(),
        [
            ".bornengine-pack.json",
            "bornengine.script.json",
            "scripts/actor.js"
        ]
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

#[test]
fn script_check_rejects_invalid_javascript_and_pack_preserves_output() {
    let package = tempfile::tempdir().unwrap();
    let manifest = write_package(
        package.path(),
        VALID_MANIFEST,
        Some(("scripts/actor.js", b"export default { update( {")),
    );
    let output = package.path().join("dist");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("keep.txt"), b"keep").unwrap();

    let checked = run_check(&manifest);
    assert!(!checked.status.success(), "{}", output_text(&checked));
    assert!(
        output_text(&checked).contains("syntax"),
        "{}",
        output_text(&checked)
    );
    let packed = run_pack(&manifest, &output);
    assert!(!packed.status.success(), "{}", output_text(&packed));
    assert_eq!(fs::read(output.join("keep.txt")).unwrap(), b"keep");
}

#[test]
fn script_check_rejects_static_imports_and_reexports() {
    let package = tempfile::tempdir().unwrap();
    let manifest = write_package(
        package.path(),
        VALID_MANIFEST,
        Some((
            "scripts/actor.js",
            b"import x from './x.js'; export default x;",
        )),
    );
    let checked = run_check(&manifest);
    assert!(!checked.status.success(), "{}", output_text(&checked));
    assert!(
        output_text(&checked).contains("module loader"),
        "{}",
        output_text(&checked)
    );

    fs::write(
        package.path().join("scripts/actor.js"),
        b"export { x } from './x.js'; export default {};",
    )
    .unwrap();
    let checked = run_check(&manifest);
    assert!(!checked.status.success(), "{}", output_text(&checked));
    assert!(
        output_text(&checked).contains("module loader"),
        "{}",
        output_text(&checked)
    );
}

#[test]
fn script_check_rejects_dynamic_import_without_running_it() {
    let package = tempfile::tempdir().unwrap();
    let manifest = write_package(
        package.path(),
        VALID_MANIFEST,
        Some((
            "scripts/actor.js",
            b"export default { update() { return import('./child.js'); } };",
        )),
    );
    let checked = run_check(&manifest);
    assert!(!checked.status.success(), "{}", output_text(&checked));
    assert!(
        output_text(&checked).contains("module loader"),
        "{}",
        output_text(&checked)
    );
}

#[test]
fn script_check_rejects_invalid_utf8_and_oversized_source() {
    let package = tempfile::tempdir().unwrap();
    let manifest = write_package(
        package.path(),
        VALID_MANIFEST,
        Some(("scripts/actor.js", &[0xff, 0xfe])),
    );
    let checked = run_check(&manifest);
    assert!(!checked.status.success(), "{}", output_text(&checked));
    assert!(
        output_text(&checked).contains("UTF-8"),
        "{}",
        output_text(&checked)
    );

    fs::write(
        package.path().join("scripts/actor.js"),
        vec![b' '; 1024 * 1024 + 1],
    )
    .unwrap();
    let checked = run_check(&manifest);
    assert!(!checked.status.success(), "{}", output_text(&checked));
    assert!(
        output_text(&checked).contains("1048576"),
        "{}",
        output_text(&checked)
    );
}

#[test]
fn script_check_and_pack_reject_deep_parentheses_without_a_signal() {
    let package = tempfile::tempdir().unwrap();
    let source = format!(
        "export default {}{}{};",
        "(".repeat(1000),
        "{}",
        ")".repeat(1000)
    );
    let manifest = write_package(
        package.path(),
        VALID_MANIFEST,
        Some(("scripts/actor.js", source.as_bytes())),
    );
    let output = package.path().join("dist");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("keep.txt"), b"keep").unwrap();

    for result in [run_check(&manifest), run_pack(&manifest, &output)] {
        assert_eq!(result.status.code(), Some(1), "{}", output_text(&result));
        assert!(
            output_text(&result).contains("nesting"),
            "{}",
            output_text(&result)
        );
    }
    assert_eq!(fs::read(output.join("keep.txt")).unwrap(), b"keep");
}

#[test]
fn script_check_and_pack_reject_oversized_manifest_without_changing_output() {
    let package = tempfile::tempdir().unwrap();
    let manifest = write_package(
        package.path(),
        &format!("{}{}", VALID_MANIFEST, " ".repeat(64 * 1024)),
        Some(("scripts/actor.js", b"export default {};")),
    );
    let output = package.path().join("dist");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("keep.txt"), b"keep").unwrap();
    let before = snapshot(&output);

    for result in [run_check(&manifest), run_pack(&manifest, &output)] {
        assert_eq!(result.status.code(), Some(1), "{}", output_text(&result));
        assert!(
            output_text(&result).contains("65536"),
            "{}",
            output_text(&result)
        );
    }
    assert_eq!(snapshot(&output), before);
}

#[test]
fn script_pack_rejects_oversized_previous_manifest_and_marker_without_changing_output() {
    for name in ["bornengine.script.json", ".bornengine-pack.json"] {
        let package = tempfile::tempdir().unwrap();
        let manifest = write_package(
            package.path(),
            VALID_MANIFEST,
            Some(("scripts/actor.js", b"export default {};")),
        );
        let output = package.path().join("dist");
        assert!(run_pack(&manifest, &output).status.success());
        let metadata_path = output.join(name);
        let mut metadata = fs::read(&metadata_path).unwrap();
        metadata.extend(vec![b' '; 64 * 1024]);
        fs::write(&metadata_path, metadata).unwrap();
        let before = snapshot(&output);

        let result = run_pack(&manifest, &output);
        assert_eq!(result.status.code(), Some(1), "{}", output_text(&result));
        assert!(
            output_text(&result).contains("65536"),
            "{}",
            output_text(&result)
        );
        assert_eq!(snapshot(&output), before);
    }
}

#[test]
fn script_pack_refuses_independently_authored_package() {
    let package = tempfile::tempdir().unwrap();
    let manifest = write_package(
        &package.path().join("source"),
        VALID_MANIFEST,
        Some(("scripts/actor.js", b"export default {};")),
    );
    let output = package.path().join("dist");
    write_package(
        &output,
        VALID_MANIFEST,
        Some(("scripts/actor.js", b"export default { update() {} };")),
    );
    let before = snapshot(&output);

    let result = run_pack(&manifest, &output);
    assert!(!result.status.success(), "{}", output_text(&result));
    assert_eq!(snapshot(&output), before);
}

#[test]
fn script_pack_refuses_modified_managed_files_and_untracked_content() {
    let package = tempfile::tempdir().unwrap();
    let manifest = write_package(
        package.path(),
        VALID_MANIFEST,
        Some(("scripts/actor.js", b"export default {};")),
    );
    let output = package.path().join("dist");
    assert!(run_pack(&manifest, &output).status.success());
    fs::write(
        output.join("scripts/actor.js"),
        b"export default { update() {} };",
    )
    .unwrap();
    let result = run_pack(&manifest, &output);
    assert!(!result.status.success(), "{}", output_text(&result));

    fs::write(output.join("scripts/actor.js"), b"export default {};").unwrap();
    fs::write(output.join("notes.txt"), b"keep").unwrap();
    let result = run_pack(&manifest, &output);
    assert!(!result.status.success(), "{}", output_text(&result));
    assert_eq!(fs::read(output.join("notes.txt")).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn script_pack_refuses_symlink_in_managed_output() {
    use std::os::unix::fs::symlink;
    let package = tempfile::tempdir().unwrap();
    let manifest = write_package(
        package.path(),
        VALID_MANIFEST,
        Some(("scripts/actor.js", b"export default {};")),
    );
    let output = package.path().join("dist");
    assert!(run_pack(&manifest, &output).status.success());
    fs::remove_file(output.join("scripts/actor.js")).unwrap();
    symlink(
        package.path().join("scripts/actor.js"),
        output.join("scripts/actor.js"),
    )
    .unwrap();
    let result = run_pack(&manifest, &output);
    assert!(!result.status.success(), "{}", output_text(&result));
    assert!(
        fs::symlink_metadata(output.join("scripts/actor.js"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
