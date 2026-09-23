use bornengine_cli::build_artifacts::{
    begin_build, begin_dev_build, clean_build_artifacts, record_build_files,
};
use std::fs;

#[test]
fn build_outputs_are_created_in_a_unique_cli_managed_directory() {
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("package.json"), "{}")
        .or_else(|_| {
            fs::create_dir_all(project.path())
                .and_then(|_| fs::write(project.path().join("package.json"), "{}"))
        })
        .unwrap();

    let first = begin_build(project.path(), "linux", "sample", None).unwrap();
    let second = begin_build(project.path(), "linux", "sample", None).unwrap();

    assert_ne!(first.directory, second.directory);
    assert!(first.output.starts_with(&first.directory));
    assert!(
        first
            .directory
            .starts_with(project.path().join(".bornengine/builds"))
    );
}

#[test]
fn clean_removes_recorded_builds_and_preserves_unrecorded_files() {
    let project = tempfile::tempdir().unwrap();
    let build = begin_build(project.path(), "linux", "sample", None).unwrap();
    fs::write(&build.output, "binary").unwrap();
    fs::write(build.directory.join("bundle.js"), "bundle").unwrap();
    record_build_files(project.path(), &build.directory).unwrap();
    let user_file = build.directory.join("notes.txt");
    fs::write(&user_file, "keep").unwrap();

    let removed = clean_build_artifacts(project.path()).unwrap();

    assert_eq!(removed, 2);
    assert!(!build.output.exists());
    assert!(!build.directory.join("bundle.js").exists());
    assert_eq!(fs::read_to_string(user_file).unwrap(), "keep");
}

#[test]
fn clean_refuses_manifest_paths_that_escape_the_build_root() {
    let project = tempfile::tempdir().unwrap();
    let marker = project.path().join("outside.txt");
    fs::write(&marker, "keep").unwrap();
    let manifest = project.path().join(".bornengine/builds/manifest.json");
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    fs::write(
        &manifest,
        r#"{"format":"bornengine-build-manifest-v1","files":["../../outside.txt"]}"#,
    )
    .unwrap();

    assert!(clean_build_artifacts(project.path()).is_err());
    assert_eq!(fs::read_to_string(marker).unwrap(), "keep");
}

#[test]
fn clean_with_no_cli_builds_is_a_noop() {
    let project = tempfile::tempdir().unwrap();
    assert_eq!(clean_build_artifacts(project.path()).unwrap(), 0);
}

#[test]
fn dev_output_uses_perry_ignored_directory_and_clean_tracks_it() {
    let project = tempfile::tempdir().unwrap();
    let dev = begin_dev_build(project.path(), "sample", None).unwrap();
    assert!(dev.directory.starts_with(project.path().join(".perry-dev")));
    fs::write(&dev.output, "binary").unwrap();
    record_build_files(project.path(), &dev.directory).unwrap();

    assert_eq!(clean_build_artifacts(project.path()).unwrap(), 1);
    assert!(!dev.output.exists());
}
