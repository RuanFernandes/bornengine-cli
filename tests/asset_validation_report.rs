use bornengine_cli::commands::assets::{
    AssetDiagnostic, AssetSeverity, AssetValidationOptions, AssetValidationReport, OrphanPolicy,
    load_asset_validation_settings, validate_project_assets_with_options,
};
use std::fs;

#[test]
fn manifest_defaults_and_cli_overrides_are_explicit() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("bornengine.assets.json"),
        r#"{"version":1,"dynamic_paths":["assets/runtime.png"],"ignored_paths":["assets/unused/**"],"orphan_severity":"error","max_file_bytes":100,"max_total_bytes":200,"max_image_dimension":64,"max_total_image_pixels":4096}"#,
    )
    .unwrap();

    let settings = load_asset_validation_settings(
        project.path(),
        &AssetValidationOptions {
            orphan_policy: Some(OrphanPolicy::Warn),
            max_file_bytes: Some(10),
            max_total_bytes: Some(20),
            max_image_dimension: Some(32),
            max_total_image_pixels: Some(1024),
        },
    )
    .unwrap();

    assert_eq!(settings.dynamic_paths, vec!["assets/runtime.png"]);
    assert_eq!(settings.ignored_paths, vec!["assets/unused/**"]);
    assert_eq!(settings.orphan_policy, OrphanPolicy::Warn);
    assert_eq!(settings.max_file_bytes, Some(10));
    assert_eq!(settings.max_total_bytes, Some(20));
    assert_eq!(settings.max_image_dimension, Some(32));
    assert_eq!(settings.max_total_image_pixels, Some(1024));
}

#[test]
fn absent_manifest_preserves_unset_budgets_and_warning_orphans() {
    let project = tempfile::tempdir().unwrap();
    let settings =
        load_asset_validation_settings(project.path(), &AssetValidationOptions::default()).unwrap();
    assert_eq!(settings.orphan_policy, OrphanPolicy::Warn);
    assert!(settings.dynamic_paths.is_empty());
    assert!(settings.ignored_paths.is_empty());
    assert_eq!(settings.max_file_bytes, None);
    assert_eq!(settings.max_total_bytes, None);
    assert_eq!(settings.max_image_dimension, None);
    assert_eq!(settings.max_total_image_pixels, None);

    fs::create_dir(project.path().join("assets")).unwrap();
    fs::write(project.path().join("assets/a.bin"), [1, 2]).unwrap();
    let report =
        validate_project_assets_with_options(project.path(), &AssetValidationOptions::default())
            .unwrap();
    assert_eq!(report.summary.files, 1);
    assert_eq!(report.summary.bytes, 2);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn invalid_manifest_entries_are_rejected() {
    let invalid = [
        (r#"{"version":2}"#, "version"),
        (
            r#"{"version":1,"dynamic_paths":["../escape.png"]}"#,
            "unsafe",
        ),
        (
            r#"{"version":1,"dynamic_paths":["/absolute.png"]}"#,
            "unsafe",
        ),
        (
            r#"{"version":1,"dynamic_paths":["C:/absolute.png"]}"#,
            "unsafe",
        ),
        (
            r#"{"version":1,"dynamic_paths":["assets/a.png","assets/a.png"]}"#,
            "duplicate",
        ),
        (
            r#"{"version":1,"ignored_paths":["assets/[ab].png"]}"#,
            "glob",
        ),
        (
            r#"{"version":1,"ignored_paths":["assets/*.png","assets/*.png"]}"#,
            "duplicate",
        ),
        (r#"{"version":1,"max_file_bytes":0}"#, "max_file_bytes"),
        (
            r#"{"version":1,"max_total_image_pixels":-1}"#,
            "max_total_image_pixels",
        ),
    ];
    for (source, expected) in invalid {
        let project = tempfile::tempdir().unwrap();
        fs::write(project.path().join("bornengine.assets.json"), source).unwrap();
        let error =
            load_asset_validation_settings(project.path(), &AssetValidationOptions::default())
                .unwrap_err()
                .to_string();
        assert!(error.contains(expected), "source={source} error={error}");
    }
}

#[test]
fn report_has_stable_sorted_project_relative_diagnostics() {
    let report = AssetValidationReport::new(
        Default::default(),
        vec![
            AssetDiagnostic {
                code: "orphan_asset".into(),
                severity: AssetSeverity::Warning,
                path: "assets/z.png".into(),
                message: "unused".into(),
                measured: None,
                limit: None,
            },
            AssetDiagnostic {
                code: "file_size_limit".into(),
                severity: AssetSeverity::Error,
                path: "assets/a.png".into(),
                message: "large".into(),
                measured: Some(11),
                limit: Some(10),
            },
        ],
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(json["diagnostics"][0]["path"], "assets/a.png");
    assert_eq!(json["diagnostics"][0]["code"], "file_size_limit");
    assert_eq!(json["diagnostics"][0]["severity"], "error");
    assert_eq!(json["diagnostics"][0]["measured"], 11);
    assert_eq!(json["diagnostics"][0]["limit"], 10);
    assert_eq!(json["diagnostics"][1]["path"], "assets/z.png");
    assert_eq!(json["diagnostics"][1]["severity"], "warning");
}

#[test]
fn report_order_is_independent_of_discovery_order_for_equal_codes_and_paths() {
    let warning = AssetDiagnostic {
        code: "asset_issue".into(),
        severity: AssetSeverity::Warning,
        path: "assets/a.png".into(),
        message: "same".into(),
        measured: None,
        limit: None,
    };
    let error = AssetDiagnostic {
        severity: AssetSeverity::Error,
        ..warning.clone()
    };
    let forward =
        AssetValidationReport::new(Default::default(), vec![warning.clone(), error.clone()]);
    let reverse = AssetValidationReport::new(Default::default(), vec![error, warning]);
    assert_eq!(
        serde_json::to_string(&forward).unwrap(),
        serde_json::to_string(&reverse).unwrap()
    );
}
