use bornengine_cli::project::{
    GameKind, ProjectSpec, create_project, find_project_root, initialize_project,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn release_spec() -> ProjectSpec {
    ProjectSpec {
        engine_package: "@bloomengine/engine".to_owned(),
        engine_spec: "0.4.16".to_owned(),
        game_kind: GameKind::TwoD,
        native_features: Vec::new(),
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
    let ai_guide = fs::read_to_string(project.join("AGENTS.md")).unwrap();
    assert!(ai_guide.starts_with("# BornEngine — AI context reference for language models"));
    assert!(ai_guide.contains("Do not pass `Game` to assets."));

    let package: Value =
        serde_json::from_slice(&fs::read(project.join("package.json")).unwrap()).unwrap();
    assert_eq!(package["name"], "my-game");
    assert_eq!(package["dependencies"]["@bloomengine/engine"], "0.4.16");
    assert_eq!(
        package["perry"]["allow"]["nativeLibrary"],
        serde_json::json!(["@bloomengine/engine/*"])
    );
    let starter = fs::read_to_string(project.join("main.ts")).unwrap();
    assert!(starter.contains("from \"@bloomengine/engine\""));
    assert!(starter.contains("extends Game"));
    assert!(starter.contains("override onStart()"));
    assert!(starter.contains("override loop(deltaTime: number)"));
    assert!(starter.contains("override render()"));
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
            game_kind: GameKind::TwoPointFiveD,
            native_features: Vec::new(),
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
            .contains("from \"@bornengine/engine\"")
    );
}

#[test]
fn generated_profile_is_owned_by_bornengine_and_readable_by_its_cli() {
    let profiles = [
        (GameKind::TwoD, "2d"),
        (GameKind::TwoPointFiveD, "2.5d"),
        (GameKind::ThreeD, "3d"),
    ];
    for (index, (game_kind, expected_features)) in profiles.iter().enumerate() {
        let parent = tempfile::tempdir().unwrap();
        let project = create_project(
            parent.path(),
            &format!("Profile{index}"),
            &ProjectSpec {
                engine_package: "@bornengine/engine".to_owned(),
                engine_spec: "0.11.0".to_owned(),
                game_kind: *game_kind,
                native_features: Vec::new(),
            },
        )
        .unwrap();
        let perry: toml::Value =
            toml::from_str(&fs::read_to_string(project.join("perry.toml")).unwrap()).unwrap();
        assert_eq!(
            perry["bornengine"]["native_profile"].as_str(),
            Some(*expected_features)
        );
        assert!(perry["bornengine"].get("native_features").is_none());
    }
}

#[test]
fn generated_perry_config_serializes_selected_native_features() {
    for (index, game_kind) in [GameKind::TwoD, GameKind::TwoPointFiveD, GameKind::ThreeD]
        .into_iter()
        .enumerate()
    {
        let parent = tempfile::tempdir().unwrap();
        let project = create_project(
            parent.path(),
            &format!("FeatureGame{index}"),
            &ProjectSpec {
                engine_package: "@bornengine/engine".to_owned(),
                engine_spec: "0.12.0".to_owned(),
                game_kind,
                native_features: vec!["sqlite".to_owned(), "scripting".to_owned()],
            },
        )
        .unwrap();
        let perry: toml::Value =
            toml::from_str(&fs::read_to_string(project.join("perry.toml")).unwrap()).unwrap();

        assert_eq!(
            perry["bornengine"]["native_profile"].as_str(),
            Some(game_kind.native_profile())
        );
        assert_eq!(
            perry["bornengine"]["native_features"].as_array().unwrap(),
            &[
                toml::Value::String("sqlite".into()),
                toml::Value::String("scripting".into())
            ]
        );
    }
}

#[test]
fn generated_project_rejects_unsupported_native_features() {
    let parent = tempfile::tempdir().unwrap();
    let error = create_project(
        parent.path(),
        "InvalidFeatureGame",
        &ProjectSpec {
            engine_package: "@bornengine/engine".to_owned(),
            engine_spec: "0.12.0".to_owned(),
            game_kind: GameKind::TwoD,
            native_features: vec!["rigid-body-magic".to_owned()],
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("native feature"));
    assert!(!parent.path().join("InvalidFeatureGame").exists());
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
fn init_does_not_add_the_new_project_ai_guide_to_existing_directories() {
    let root = tempfile::tempdir().unwrap();

    initialize_project(root.path(), "ExistingGame", &release_spec()).unwrap();

    assert!(!root.path().join("AGENTS.md").exists());
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
