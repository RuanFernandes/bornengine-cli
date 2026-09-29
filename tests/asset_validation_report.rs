use bornengine_cli::commands::assets::{
    AssetDiagnostic, AssetDiagnosticCode, AssetSeverity, AssetValidationOptions,
    AssetValidationReport, OrphanPolicy, load_asset_validation_settings, validate_project_assets,
    validate_project_assets_with_options,
};
use std::fs;
use std::path::Path;

#[test]
fn manifest_defaults_and_cli_overrides_are_explicit() {
    let project = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/asset_audit/complete"
    ));

    let manifest =
        load_asset_validation_settings(project, &AssetValidationOptions::default()).unwrap();
    assert_eq!(manifest.dynamic_paths, vec!["assets/dynamic.png"]);
    assert_eq!(manifest.ignored_paths, vec!["assets/ignored/**"]);
    assert_eq!(manifest.orphan_policy, OrphanPolicy::Error);
    assert_eq!(manifest.max_file_bytes, Some(128));
    assert_eq!(manifest.max_total_bytes, Some(400));
    assert_eq!(manifest.max_image_dimension, Some(2));
    assert_eq!(manifest.max_total_image_pixels, Some(8));

    let settings = load_asset_validation_settings(
        project,
        &AssetValidationOptions {
            orphan_policy: Some(OrphanPolicy::Warn),
            max_file_bytes: Some(10),
            max_total_bytes: Some(20),
            max_image_dimension: Some(32),
            max_total_image_pixels: Some(1024),
        },
    )
    .unwrap();

    assert_eq!(settings.dynamic_paths, vec!["assets/dynamic.png"]);
    assert_eq!(settings.ignored_paths, vec!["assets/ignored/**"]);
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
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, AssetDiagnosticCode::OrphanAsset);
    assert_eq!(report.diagnostics[0].severity, AssetSeverity::Warning);
}

#[test]
fn invalid_manifest_entries_are_rejected() {
    let invalid = [
        (r#"{"version":2}"#, "version"),
        (
            include_str!("fixtures/asset_audit/invalid-traversal/bornengine.assets.json"),
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
            include_str!("fixtures/asset_audit/invalid-glob/bornengine.assets.json"),
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
                code: AssetDiagnosticCode::OrphanAsset,
                severity: AssetSeverity::Warning,
                path: "assets/z.png".into(),
                message: "unused".into(),
                measured: None,
                limit: None,
            },
            AssetDiagnostic {
                code: AssetDiagnosticCode::FileSizeLimit,
                severity: AssetSeverity::Error,
                path: "assets/a.png".into(),
                message: "large".into(),
                measured: Some(11),
                limit: Some(10),
            },
        ],
    )
    .unwrap();
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
        code: AssetDiagnosticCode::OrphanAsset,
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
        AssetValidationReport::new(Default::default(), vec![warning.clone(), error.clone()])
            .unwrap();
    let reverse = AssetValidationReport::new(Default::default(), vec![error, warning]).unwrap();
    assert_eq!(
        serde_json::to_string(&forward).unwrap(),
        serde_json::to_string(&reverse).unwrap()
    );
}

#[test]
fn diagnostic_codes_have_a_stable_complete_json_vocabulary() {
    let cases = [
        (AssetDiagnosticCode::OrphanAsset, "orphan_asset"),
        (
            AssetDiagnosticCode::InvalidMediaHeader,
            "invalid_media_header",
        ),
        (AssetDiagnosticCode::ExtensionMismatch, "extension_mismatch"),
        (AssetDiagnosticCode::FileSizeLimit, "file_size_limit"),
        (AssetDiagnosticCode::TotalSizeLimit, "total_size_limit"),
        (
            AssetDiagnosticCode::ImageDimensionLimit,
            "image_dimension_limit",
        ),
        (
            AssetDiagnosticCode::TotalImagePixelsLimit,
            "total_image_pixels_limit",
        ),
        (
            AssetDiagnosticCode::DeclaredDynamicPathMissing,
            "declared_dynamic_path_missing",
        ),
    ];
    for (code, expected) in cases {
        assert_eq!(
            serde_json::to_string(&code).unwrap(),
            format!("\"{expected}\"")
        );
    }
}

#[test]
fn report_rejects_unsafe_or_platform_specific_diagnostic_paths() {
    for path in [
        "../escape.png",
        "/absolute.png",
        "C:/absolute.png",
        "assets\\item.png",
        "assets//item.png",
    ] {
        let diagnostic = AssetDiagnostic {
            code: AssetDiagnosticCode::OrphanAsset,
            severity: AssetSeverity::Warning,
            path: path.into(),
            message: "unused".into(),
            measured: None,
            limit: None,
        };
        assert!(
            AssetValidationReport::new(Default::default(), vec![diagnostic]).is_err(),
            "accepted {path}"
        );
    }
}

#[test]
fn report_json_schema_excludes_host_paths_and_has_stable_fields() {
    let summary = bornengine_cli::commands::assets::AssetSummary {
        files: 2,
        bytes: 10,
        watch_directories: vec![Path::new("/host/private/assets").to_path_buf()],
    };
    let report = AssetValidationReport::new(
        summary,
        vec![AssetDiagnostic {
            code: AssetDiagnosticCode::FileSizeLimit,
            severity: AssetSeverity::Error,
            path: "assets/a.png".into(),
            message: "asset exceeds file byte limit".into(),
            measured: Some(6),
            limit: Some(5),
        }],
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&report).unwrap(),
        r#"{"format":"bornengine.asset_validation","version":1,"summary":{"files":2,"bytes":10},"diagnostics":[{"code":"file_size_limit","severity":"error","path":"assets/a.png","message":"asset exceeds file byte limit","measured":6,"limit":5}]}"#
    );
}

#[cfg(unix)]
#[test]
fn audit_manifest_symlink_cannot_read_outside_the_project() {
    use std::os::unix::fs::symlink;
    let project = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("manifest.json"), r#"{"version":1}"#).unwrap();
    symlink(
        outside.path().join("manifest.json"),
        project.path().join("bornengine.assets.json"),
    )
    .unwrap();
    let error = load_asset_validation_settings(project.path(), &AssetValidationOptions::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("outside the project root"), "{error}");
}

#[test]
fn checked_in_audit_fixture_preserves_the_existing_safe_inventory() {
    let project = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/asset_audit/complete"
    ));
    let summary = validate_project_assets_with_options(project, &AssetValidationOptions::default())
        .unwrap()
        .summary;
    assert_eq!(summary.files, 10);
    assert_eq!(summary.bytes, 650);
}

fn audit_fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/asset_audit")
        .join(name)
}

#[test]
fn audit_fixture_reports_only_unreferenced_non_ignored_assets() {
    let report = validate_project_assets_with_options(
        &audit_fixture("complete"),
        &AssetValidationOptions::default(),
    )
    .unwrap();
    let orphans = report
        .diagnostics
        .iter()
        .filter(|item| item.code == AssetDiagnosticCode::OrphanAsset)
        .map(|item| (item.path.as_str(), item.severity))
        .collect::<Vec<_>>();
    assert_eq!(
        orphans,
        [
            ("assets/large.bin", AssetSeverity::Error),
            ("assets/malformed.png", AssetSeverity::Error),
            ("assets/malformed.wav", AssetSeverity::Error),
            ("assets/mismatch.jpg", AssetSeverity::Error),
            ("assets/mismatch.mp3", AssetSeverity::Error),
            ("assets/orphan.wav", AssetSeverity::Error),
            ("assets/wide.png", AssetSeverity::Error),
        ]
    );
    assert!(!report.diagnostics.iter().any(|item| {
        [
            "assets/referenced.png",
            "assets/dynamic.png",
            "assets/ignored/skip.png",
        ]
        .contains(&item.path.as_str())
    }));
    assert!(report.has_errors());
}

#[test]
fn audit_fixture_detects_bad_media_and_budget_measurements() {
    let report = validate_project_assets_with_options(
        &audit_fixture("complete"),
        &AssetValidationOptions::default(),
    )
    .unwrap();
    let cases = [
        (
            "assets/malformed.png",
            AssetDiagnosticCode::InvalidMediaHeader,
            None,
            None,
        ),
        (
            "assets/malformed.wav",
            AssetDiagnosticCode::InvalidMediaHeader,
            None,
            None,
        ),
        (
            "assets/mismatch.jpg",
            AssetDiagnosticCode::ExtensionMismatch,
            None,
            None,
        ),
        (
            "assets/mismatch.mp3",
            AssetDiagnosticCode::ExtensionMismatch,
            None,
            None,
        ),
        (
            "assets/large.bin",
            AssetDiagnosticCode::FileSizeLimit,
            Some(192),
            Some(128),
        ),
        (
            "assets/wide.png",
            AssetDiagnosticCode::ImageDimensionLimit,
            Some(4),
            Some(2),
        ),
        (
            "",
            AssetDiagnosticCode::TotalImagePixelsLimit,
            Some(11),
            Some(8),
        ),
        (
            "",
            AssetDiagnosticCode::TotalSizeLimit,
            Some(581),
            Some(400),
        ),
    ];
    for (path, code, measured, limit) in cases {
        assert!(
            report.diagnostics.iter().any(|item| {
                item.path == path
                    && item.code == code
                    && item.measured == measured
                    && item.limit == limit
            }),
            "missing {code:?} for {path}: {:?}",
            report.diagnostics
        );
    }
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|item| item.path == "assets/ignored/skip.png")
    );
    assert!(report.diagnostics.windows(2).all(|pair| {
        (pair[0].path.as_str(), pair[0].code) <= (pair[1].path.as_str(), pair[1].code)
    }));
}

#[test]
fn missing_dynamic_declaration_is_reported_as_error() {
    let report = validate_project_assets_with_options(
        &audit_fixture("missing-dynamic"),
        &AssetValidationOptions::default(),
    )
    .unwrap();
    assert!(report.diagnostics.iter().any(|item| {
        item.code == AssetDiagnosticCode::DeclaredDynamicPathMissing
            && item.path == "assets/does-not-exist.png"
            && item.severity == AssetSeverity::Error
    }));
}

#[test]
fn default_validation_observes_configured_error_severity() {
    let error = validate_project_assets(&audit_fixture("complete"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("orphan_asset"), "{error}");
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("assets")).unwrap();
    fs::write(project.path().join("assets/unused.png"), b"bad").unwrap();
    assert!(validate_project_assets(project.path()).is_ok());
}

#[test]
fn audit_settings_leave_pack_manifest_and_payload_bytes_unchanged() {
    let fixture = audit_fixture("complete");
    let project = tempfile::tempdir().unwrap();
    let plain_output = tempfile::tempdir().unwrap();
    let audit_output = tempfile::tempdir().unwrap();
    let packed_paths = [
        "assets/dynamic.png",
        "assets/ignored/skip.png",
        "assets/large.bin",
        "assets/malformed.png",
        "assets/malformed.wav",
        "assets/mismatch.jpg",
        "assets/mismatch.mp3",
        "assets/orphan.wav",
        "assets/referenced.png",
        "assets/wide.png",
    ];
    for relative in packed_paths
        .iter()
        .copied()
        .chain(["worlds/level.world2d.json"])
    {
        let destination = project.path().join(relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(fixture.join(relative), destination).unwrap();
    }
    bornengine_cli::commands::assets::pack_project(project.path(), plain_output.path()).unwrap();
    bornengine_cli::commands::assets::pack_project(&fixture, audit_output.path()).unwrap();
    assert_eq!(
        fs::read(plain_output.path().join("assets.manifest.json")).unwrap(),
        fs::read(audit_output.path().join("assets.manifest.json")).unwrap()
    );
    for relative in packed_paths {
        assert_eq!(
            fs::read(plain_output.path().join(relative)).unwrap(),
            fs::read(audit_output.path().join(relative)).unwrap()
        );
    }
}
