use bornengine_cli::commands::assets::{pack_project, validate_project_assets};
use std::fs;

#[test]
fn asset_manifest_is_stable_sorted_and_pack_copies_exact_bytes() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("assets")).unwrap();
    fs::write(project.path().join("assets/z.bin"), [9, 8, 7]).unwrap();
    fs::write(project.path().join("assets/a.bin"), [1, 2, 3, 4]).unwrap();

    let output = tempfile::tempdir().unwrap();
    let first = pack_project(project.path(), output.path()).unwrap();
    let manifest_first = fs::read(output.path().join("assets.manifest.json")).unwrap();
    let second = pack_project(project.path(), output.path()).unwrap();
    let manifest_second = fs::read(output.path().join("assets.manifest.json")).unwrap();

    assert_eq!(first.files, 2);
    assert_eq!(second.files, 2);
    assert_eq!(manifest_first, manifest_second);
    assert_eq!(
        fs::read(output.path().join("assets/a.bin")).unwrap(),
        [1, 2, 3, 4]
    );
    let manifest = String::from_utf8(manifest_first).unwrap();
    assert!(manifest.find("assets/a.bin").unwrap() < manifest.find("assets/z.bin").unwrap());
    assert!(manifest.contains("sha256"));
}

#[test]
fn repacking_removes_only_stale_files_owned_by_the_previous_manifest() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("assets")).unwrap();
    fs::write(project.path().join("assets/keep.bin"), [1]).unwrap();
    fs::write(project.path().join("assets/remove.bin"), [2]).unwrap();
    let output = tempfile::tempdir().unwrap();
    pack_project(project.path(), output.path()).unwrap();
    fs::write(output.path().join("notes.txt"), "user file").unwrap();

    fs::remove_file(project.path().join("assets/remove.bin")).unwrap();
    pack_project(project.path(), output.path()).unwrap();

    assert!(output.path().join("assets/keep.bin").exists());
    assert!(!output.path().join("assets/remove.bin").exists());
    assert_eq!(
        fs::read_to_string(output.path().join("notes.txt")).unwrap(),
        "user file"
    );
}

#[test]
fn rejects_referenced_asset_path_escape_and_preserves_clear_diagnostic() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("maps")).unwrap();
    fs::write(
        project.path().join("maps/level.world2d.json"),
        r#"{"format":"bornengine.world2d","version":1,"assets":["../../outside.png"]}"#,
    )
    .unwrap();

    let error = validate_project_assets(project.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("level.world2d.json"), "{error}");
    assert!(error.contains("outside"), "{error}");
}

#[test]
fn validates_missing_and_case_mismatched_references_with_document_context() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("assets/images")).unwrap();
    fs::write(project.path().join("assets/images/Player.PNG"), [1, 2, 3]).unwrap();
    fs::create_dir_all(project.path().join("worlds")).unwrap();
    let world = project.path().join("worlds/level.world2d.json");
    fs::write(
        &world,
        r#"{"format":"bornengine.world2d","version":1,"assets":["assets/images/player.png"]}"#,
    )
    .unwrap();

    let error = validate_project_assets(project.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("level.world2d.json"), "{error}");
    assert!(
        error.contains("did you mean `assets/images/Player.PNG`"),
        "{error}"
    );

    fs::write(
        &world,
        r#"{"format":"bornengine.world2d","version":1,"assets":["assets/images/absent.png"]}"#,
    )
    .unwrap();
    let error = validate_project_assets(project.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("assets/images/absent.png"), "{error}");
    assert!(error.contains("does not exist"), "{error}");
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_that_escape_project_root() {
    use std::os::unix::fs::symlink;

    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    fs::write(external.path().join("secret.png"), [1]).unwrap();
    fs::create_dir_all(project.path().join("assets")).unwrap();
    symlink(
        external.path().join("secret.png"),
        project.path().join("assets/linked.png"),
    )
    .unwrap();

    assert!(pack_project(project.path(), &project.path().join("packed")).is_err());
}
