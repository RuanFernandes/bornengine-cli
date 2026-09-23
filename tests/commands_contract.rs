use bornengine_cli::commands::build::{derive_output_name, validate_run_target};
use bornengine_cli::platform::{BuildTarget, HostPlatform, ResolvedTarget};
use serde_json::json;
use std::fs;
use std::path::Path;

#[test]
fn output_name_prefers_explicit_then_project_then_entry_name() {
    let root = tempfile::tempdir().unwrap();
    let entry = root.path().join("src/main.ts");
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(&entry, "").unwrap();
    let package = json!({"name":"game-project"});

    assert_eq!(
        derive_output_name(Some("custom-build"), &package, &entry).unwrap(),
        "custom-build"
    );
    assert_eq!(
        derive_output_name(None, &package, &entry).unwrap(),
        "game-project"
    );
    assert_eq!(
        derive_output_name(None, &json!({}), &entry).unwrap(),
        "main"
    );
}

#[test]
fn output_name_rejects_paths_and_empty_names() {
    let entry = Path::new("main.ts");
    assert!(derive_output_name(Some("../outside"), &json!({}), entry).is_err());
    assert!(derive_output_name(Some(""), &json!({}), entry).is_err());
}

#[test]
fn run_validation_rejects_non_host_targets_before_process_execution() {
    let target = ResolvedTarget {
        target: BuildTarget::Windows,
        perry_target: Some("windows".to_owned()),
    };
    assert!(validate_run_target(&target, HostPlatform::Linux).is_err());
    assert!(validate_run_target(&target, HostPlatform::Windows).is_ok());
}
