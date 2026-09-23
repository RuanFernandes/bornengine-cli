use bornengine_cli::config::Config;
use bornengine_cli::engine::{
    EngineDependency, EngineRelease, engine_package_from_manifest, local_engine_dependency,
    remove_engine_dependency, select_engine_release, set_engine_dependency,
};
use bornengine_cli::package_manager::PackageManager;
use serde_json::json;
use std::fs;

#[test]
fn config_defaults_to_pnpm_and_latest_engine() {
    assert_eq!(Config::default().package_manager, "pnpm");
    assert_eq!(Config::default().engine_version, "latest");
}

#[test]
fn config_round_trips_supported_values_as_toml() {
    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join("config.toml");
    let mut config = Config::load(&config_path).unwrap();
    config.set("package-manager", "npm").unwrap();
    config.set("engine-version", "0.5.0").unwrap();
    config.save(&config_path).unwrap();

    let loaded = Config::load(&config_path).unwrap();
    assert_eq!(loaded.get("package-manager").as_deref(), Some("npm"));
    assert_eq!(loaded.get("engine-version").as_deref(), Some("0.5.0"));
    assert!(
        fs::read_to_string(config_path)
            .unwrap()
            .contains("package_manager = \"npm\"")
    );
}

#[test]
fn rejects_unknown_config_keys_and_invalid_package_managers() {
    let mut config = Config::default();
    assert!(config.set("other", "value").is_err());
    assert!(config.set("package-manager", "bun").is_err());
}

#[test]
fn selects_latest_stable_release_and_prefers_the_new_namespace() {
    let bornengine = json!({
        "dist-tags": {"latest": "1.0.0-beta.1"},
        "versions": {"0.9.0": {}, "1.0.0-beta.1": {}, "0.10.0": {}}
    });
    let legacy = json!({
        "dist-tags": {"latest": "0.8.0"},
        "versions": {"0.8.0": {}}
    });

    let release = select_engine_release(Some(&bornengine), Some(&legacy), None).unwrap();

    assert_eq!(
        release,
        EngineRelease {
            package_name: "@bornengine/engine".to_owned(),
            version: "0.10.0".to_owned()
        }
    );
}

#[test]
fn falls_back_to_legacy_package_when_new_namespace_is_unpublished() {
    let legacy = json!({
        "dist-tags": {"latest": "0.4.16"},
        "versions": {"0.4.16": {}}
    });

    let release = select_engine_release(None, Some(&legacy), None).unwrap();

    assert_eq!(release.package_name, "@bloomengine/engine");
    assert_eq!(release.version, "0.4.16");
}

#[test]
fn selects_exact_stable_version_only_if_it_exists() {
    let metadata = json!({"versions": {"0.5.0": {}, "0.6.0-rc.1": {}}});
    let release = select_engine_release(Some(&metadata), None, Some("0.5.0")).unwrap();
    assert_eq!(release.version, "0.5.0");
    assert_eq!(
        select_engine_release(Some(&metadata), None, Some("latest"))
            .unwrap()
            .version,
        "0.5.0"
    );
    assert!(select_engine_release(Some(&metadata), None, Some("0.6.0-rc.1")).is_err());
}

#[test]
fn recognizes_only_current_or_legacy_engine_package_names() {
    assert_eq!(
        engine_package_from_manifest(&json!({"name":"@bornengine/engine"})).unwrap(),
        "@bornengine/engine"
    );
    assert_eq!(
        engine_package_from_manifest(&json!({"name":"@bloomengine/engine"})).unwrap(),
        "@bloomengine/engine"
    );
    assert!(engine_package_from_manifest(&json!({"name":"other"})).is_err());
}

#[test]
fn missing_engine_metadata_has_an_actionable_registry_error() {
    let error = select_engine_release(None, None, None).unwrap_err();
    assert!(error.to_string().contains("npm registry"));
}

#[test]
fn local_checkout_uses_the_package_name_and_package_manager_path_protocol() {
    let parent = tempfile::tempdir().unwrap();
    let engine = parent.path().join("BornEngine");
    let project = parent.path().join("TestGame");
    fs::create_dir(&engine).unwrap();
    fs::create_dir(&project).unwrap();
    fs::write(
        engine.join("package.json"),
        r#"{"name":"@bornengine/engine","version":"0.5.0"}"#,
    )
    .unwrap();

    let pnpm = local_engine_dependency(&engine, &project, PackageManager::Pnpm).unwrap();
    let npm = local_engine_dependency(&engine, &project, PackageManager::Npm).unwrap();
    let yarn = local_engine_dependency(&engine, &project, PackageManager::Yarn).unwrap();

    assert_eq!(pnpm.package_name, "@bornengine/engine");
    assert_eq!(pnpm.spec, "link:../BornEngine");
    assert_eq!(npm.spec, "file:../BornEngine");
    assert_eq!(yarn.spec, "file:../BornEngine");
}

#[test]
fn engine_dependency_update_preserves_other_dependencies_and_remove_is_scoped() {
    let mut package = json!({
        "dependencies": {"@bloomengine/engine":"0.4.16", "left-pad":"1.3.0"},
        "devDependencies": {"test-tool":"2.0.0"}
    });

    set_engine_dependency(
        &mut package,
        &EngineDependency {
            package_name: "@bornengine/engine".to_owned(),
            spec: "0.5.0".to_owned(),
        },
    )
    .unwrap();
    assert_eq!(package["dependencies"]["@bornengine/engine"], "0.5.0");
    assert!(package["dependencies"].get("@bloomengine/engine").is_none());
    assert_eq!(package["dependencies"]["left-pad"], "1.3.0");

    let removed = remove_engine_dependency(&mut package).unwrap().unwrap();
    assert_eq!(removed.package_name, "@bornengine/engine");
    assert!(package["dependencies"].get("@bornengine/engine").is_none());
    assert_eq!(package["dependencies"]["left-pad"], "1.3.0");
}
