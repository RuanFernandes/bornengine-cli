use bornengine_cli::project::{ProjectSpec, create_project};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn run_validate(root: &Path, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .args(["assets", "validate"])
        .arg(root)
        .args(flags)
        .output()
        .unwrap()
}

fn project_with_orphan(severity: &str) -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("assets")).unwrap();
    fs::write(project.path().join("assets/unused.txt"), "unused").unwrap();
    fs::write(
        project.path().join("bornengine.assets.json"),
        json!({"version": 1, "orphan_severity": severity}).to_string(),
    )
    .unwrap();
    project
}

#[test]
fn json_stdout_is_one_versioned_report_with_no_human_text() {
    let project = project_with_orphan("warning");
    let output = run_validate(project.path(), &["--json"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["format"], "bornengine.asset_validation");
    assert_eq!(report["version"], 1);
    assert_eq!(report["summary"], json!({"files": 1, "bytes": 6}));
    assert_eq!(report["diagnostics"][0]["code"], "orphan_asset");
    assert_eq!(report["diagnostics"][0]["path"], "assets/unused.txt");
}

#[test]
fn literal_glob_characters_in_asset_names_remain_valid_diagnostic_paths() {
    let project = tempfile::tempdir().unwrap();
    let assets = project.path().join("assets");
    fs::create_dir(&assets).unwrap();
    fs::write(assets.join("icon[1].txt"), "literal bracket path").unwrap();
    fs::write(assets.join("data{old}.bin"), "literal brace path").unwrap();
    fs::write(
        project.path().join("bornengine.assets.json"),
        r#"{"version":1}"#,
    )
    .unwrap();

    let output = run_validate(project.path(), &["--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let paths = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["assets/data{old}.bin", "assets/icon[1].txt"]);
}

#[test]
fn dynamic_paths_treat_glob_metacharacters_as_literal_file_names() {
    let project = tempfile::tempdir().unwrap();
    let assets = project.path().join("assets");
    fs::create_dir(&assets).unwrap();
    fs::write(assets.join("icon[1].txt"), "dynamic file").unwrap();
    fs::write(
        project.path().join("bornengine.assets.json"),
        r#"{"version":1,"dynamic_paths":["assets/icon[1].txt"],"orphan_severity":"error"}"#,
    )
    .unwrap();

    let output = run_validate(project.path(), &["--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"], json!([]));
}

#[test]
fn complete_audit_fixture_emits_only_one_json_report() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/asset_audit/complete");
    let output = run_validate(&fixture, &["--json"]);
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["format"], "bornengine.asset_validation");
    assert_eq!(report["version"], 1);
    assert_eq!(report["summary"], json!({"files": 10, "bytes": 650}));
    assert!(
        report["diagnostics"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
}

#[test]
fn json_report_is_byte_stable_across_repeated_runs() {
    let project = project_with_orphan("warning");
    let first = run_validate(project.path(), &["--json"]);
    let second = run_validate(project.path(), &["--json"]);
    let _: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn human_mode_prints_diagnostics_to_stderr_and_existing_summary_to_stdout() {
    let project = project_with_orphan("warning");
    let output = run_validate(project.path(), &[]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Validated 1 project assets (6 bytes)\n"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("warning"), "{stderr}");
    assert!(stderr.contains("orphan_asset"), "{stderr}");
    assert!(stderr.contains("assets/unused.txt"), "{stderr}");
}

#[test]
fn error_diagnostics_make_both_output_modes_fail() {
    let project = project_with_orphan("error");
    for flags in [Vec::<&str>::new(), vec!["--json"]] {
        let output = run_validate(project.path(), &flags);
        assert!(!output.status.success());
        if flags.is_empty() {
            assert!(String::from_utf8_lossy(&output.stderr).contains("orphan_asset"));
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "Validated 1 project assets (6 bytes)\n"
            );
        } else {
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["diagnostics"][0]["severity"], "error");
            assert!(output.stderr.is_empty());
        }
    }
}

#[test]
fn command_flags_override_manifest_policy_and_limits() {
    let project = project_with_orphan("error");
    let output = run_validate(project.path(), &["--json", "--orphan-policy", "ignore"]);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"], json!([]));

    let output = run_validate(
        project.path(),
        &[
            "--json",
            "--orphan-policy",
            "ignore",
            "--max-file-bytes",
            "5",
        ],
    );
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "file_size_limit");
    assert_eq!(report["diagnostics"][0]["limit"], 5);
}

#[test]
fn generated_starter_uses_game_subclass_and_passes_asset_validation() {
    let parent = tempfile::tempdir().unwrap();
    let project = create_project(
        parent.path(),
        "Starter",
        &ProjectSpec {
            engine_package: "@bornengine/engine".to_owned(),
            engine_spec: "0.10.0".to_owned(),
        },
    )
    .unwrap();
    let source = fs::read_to_string(project.join("main.ts")).unwrap();
    assert!(source.contains("from \"@bornengine/engine\""));
    for token in ["extends Game", "onStart()", "loop(", "render()", ".run()"] {
        assert!(source.contains(token), "missing {token}: {source}");
    }
    assert!(!source.contains("initWindow"));
    assert!(!source.contains("runGame"));
    let output = run_validate(&project, &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
