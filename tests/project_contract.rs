use bornengine_cli::project::{ProjectSpec, create_project, find_project_root, initialize_project};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn release_spec() -> ProjectSpec {
    ProjectSpec {
        engine_package: "@bloomengine/engine".to_owned(),
        engine_spec: "0.4.16".to_owned(),
    }
}

#[test]
fn creates_small_game_project_with_pinned_engine_and_perry_allowlist() {
    let parent = tempfile::tempdir().unwrap();
    let project = create_project(parent.path(), "MyGame", &release_spec()).unwrap();

    assert_eq!(project, parent.path().join("MyGame"));
    assert!(project.join("main.ts").is_file());
    assert!(project.join(".gitignore").is_file());
    assert!(project.join("README.md").is_file());

    let package: Value =
        serde_json::from_slice(&fs::read(project.join("package.json")).unwrap()).unwrap();
    assert_eq!(package["name"], "my-game");
    assert_eq!(package["dependencies"]["@bloomengine/engine"], "0.4.16");
    assert_eq!(
        package["perry"]["allow"]["nativeLibrary"],
        serde_json::json!(["@bloomengine/engine/*"])
    );
    let starter = fs::read_to_string(project.join("main.ts")).unwrap();
    assert!(starter.contains("@bloomengine/engine/core"));
    assert!(starter.contains("@bloomengine/engine/shapes"));
}

#[test]
fn local_engine_spec_is_kept_in_package_json_and_allowlist() {
    let parent = tempfile::tempdir().unwrap();
    let project = create_project(
        parent.path(),
        "LocalGame",
        &ProjectSpec {
            engine_package: "@bornengine/engine".to_owned(),
            engine_spec: "link:../BornEngine".to_owned(),
        },
    )
    .unwrap();
    let package: Value =
        serde_json::from_slice(&fs::read(project.join("package.json")).unwrap()).unwrap();

    assert_eq!(
        package["dependencies"]["@bornengine/engine"],
        "link:../BornEngine"
    );
    assert_eq!(
        package["perry"]["allow"]["nativeLibrary"],
        serde_json::json!(["@bornengine/engine/*"])
    );
    assert!(
        fs::read_to_string(project.join("main.ts"))
            .unwrap()
            .contains("@bornengine/engine/core")
    );
}

#[test]
fn rejects_project_names_that_can_escape_the_parent_directory() {
    let parent = tempfile::tempdir().unwrap();
    let error = create_project(parent.path(), "../outside", &release_spec()).unwrap_err();

    assert!(error.to_string().contains("single directory name"));
    assert!(!parent.path().parent().unwrap().join("outside").exists());
}

#[test]
fn refuses_populated_directories_without_changing_existing_files() {
    let parent = tempfile::tempdir().unwrap();
    let target = parent.path().join("MyGame");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("notes.txt"), "keep me").unwrap();

    let error = create_project(parent.path(), "MyGame", &release_spec()).unwrap_err();

    assert!(error.to_string().contains("already contains files"));
    assert_eq!(
        fs::read_to_string(target.join("notes.txt")).unwrap(),
        "keep me"
    );
    assert!(!target.join("package.json").exists());
}

#[test]
fn init_refuses_a_conflicting_file_without_overwriting_it() {
    let root = tempfile::tempdir().unwrap();
    let package_file = root.path().join("package.json");
    fs::write(&package_file, "{\"name\":\"my-app\"}").unwrap();

    let error = initialize_project(root.path(), "my-app", &release_spec()).unwrap_err();

    assert!(error.to_string().contains("already exists"));
    assert_eq!(
        fs::read_to_string(package_file).unwrap(),
        "{\"name\":\"my-app\"}"
    );
    assert!(!root.path().join("main.ts").exists());
}

#[test]
fn discovers_project_root_from_a_nested_source_directory() {
    let parent = tempfile::tempdir().unwrap();
    let project = create_project(parent.path(), "NestedGame", &release_spec()).unwrap();
    let nested = project.join("src").join("deep");
    fs::create_dir_all(&nested).unwrap();

    assert_eq!(find_project_root(&nested).unwrap(), Some(project));
}

#[test]
fn does_not_treat_an_unrelated_package_as_a_bornengine_project() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("package.json"),
        r#"{"name":"other-tool","dependencies":{"left-pad":"1.0.0"}}"#,
    )
    .unwrap();

    assert_eq!(find_project_root(Path::new(root.path())).unwrap(), None);
}

#[test]
fn malformed_project_metadata_is_reported_instead_of_being_silently_skipped() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("package.json"), "{not json").unwrap();

    let error = find_project_root(root.path()).unwrap_err();

    assert!(error.to_string().contains("invalid JSON"));
}
