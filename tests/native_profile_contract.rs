use bornengine_cli::cargo_profile::{args_with_profile, read_native_features, read_native_profile};
use bornengine_cli::project::GameKind;
use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::process::Command;

#[test]
fn profile_is_read_from_bornengine_metadata_in_perry_toml() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("perry.toml"),
        "[project]\nname = \"test\"\n\n[bornengine]\nnative_profile = \"2.5d\"\n",
    )
    .unwrap();

    assert_eq!(
        read_native_profile(project.path()).unwrap(),
        GameKind::TwoPointFiveD
    );
}

#[test]
fn user_features_are_added_to_the_selected_profile_without_replacing_it() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("perry.toml"),
        "[bornengine]\nnative_profile = \"2d\"\nnative_features = [\"debug-ui\"]\n",
    )
    .unwrap();

    assert_eq!(
        read_native_features(project.path()).unwrap(),
        ["mp3", "debug-ui"]
    );
}

#[test]
fn sqlite_and_scripting_are_added_to_all_supported_profiles() {
    for (profile, expected) in [
        ("2d", ["mp3", "sqlite", "scripting"].as_slice()),
        (
            "2.5d",
            ["mp3", "models3d", "image-extras", "sqlite", "scripting"].as_slice(),
        ),
        (
            "3d",
            [
                "mp3",
                "jolt",
                "models3d",
                "image-extras",
                "sqlite",
                "scripting",
            ]
            .as_slice(),
        ),
    ] {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join("perry.toml"),
            format!(
                "[bornengine]\nnative_profile = \"{profile}\"\nnative_features = [\"sqlite\", \"scripting\"]\n"
            ),
        )
        .unwrap();

        assert_eq!(read_native_features(project.path()).unwrap(), expected);
    }
}

#[test]
fn all_native_profiles_resolve_to_the_documented_cargo_features() {
    let profiles: [(&str, &[&str]); 3] = [
        ("2d", &["mp3"]),
        ("2.5d", &["mp3", "models3d", "image-extras"]),
        ("3d", &["mp3", "jolt", "models3d", "image-extras"]),
    ];
    for (profile, expected) in profiles {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join("perry.toml"),
            format!("[bornengine]\nnative_profile = \"{profile}\"\n"),
        )
        .unwrap();

        assert_eq!(read_native_features(project.path()).unwrap(), expected);
    }
}

#[test]
fn explicit_profile_wins_over_stale_legacy_feature_configuration() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("perry.toml"),
        "[bornengine]\nnative_profile = \"2d\"\n\n[native-library.\"@bornengine/engine\"]\nfeatures = [\"jolt\", \"models3d\"]\n",
    )
    .unwrap();

    assert_eq!(read_native_features(project.path()).unwrap(), ["mp3"]);
}

#[test]
fn engine_cargo_build_gets_only_selected_native_features() {
    let project = tempfile::tempdir().unwrap();
    let engine = project.path().join("node_modules/@bornengine/engine");
    let manifest = engine.join("native/linux/Cargo.toml");
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    fs::write(&manifest, "[package]\nname = \"engine\"\n").unwrap();

    let args = vec![
        OsString::from("build"),
        OsString::from("--release"),
        OsString::from("--manifest-path"),
        manifest.canonicalize().unwrap().into_os_string(),
    ];
    let adjusted = args_with_profile(&args, &engine, GameKind::TwoD.native_features());

    assert_eq!(
        adjusted,
        [
            "build",
            "--release",
            "--manifest-path",
            manifest.canonicalize().unwrap().to_str().unwrap(),
            "--no-default-features",
            "--features",
            "mp3",
        ]
        .map(OsString::from)
    );
}

#[test]
fn unrelated_cargo_build_does_not_receive_engine_features() {
    let project = tempfile::tempdir().unwrap();
    let engine = project.path().join("engine");
    let other_manifest = project.path().join("game/Cargo.toml");
    fs::create_dir_all(other_manifest.parent().unwrap()).unwrap();
    fs::write(&other_manifest, "[package]\nname = \"game\"\n").unwrap();
    let args = vec![
        OsString::from("build"),
        OsString::from("--manifest-path"),
        other_manifest.canonicalize().unwrap().into_os_string(),
    ];

    assert_eq!(args_with_profile(&args, &engine, &["mp3"]), args);
}

#[test]
fn native_cargo_manifest_outside_engine_root_is_not_profiled() {
    let project = tempfile::tempdir().unwrap();
    let engine = project.path().join("engine");
    let unrelated_manifest = project.path().join("vendor/native/linux/Cargo.toml");
    fs::create_dir_all(unrelated_manifest.parent().unwrap()).unwrap();
    fs::write(&unrelated_manifest, "[package]\nname = \"vendor\"\n").unwrap();
    let args = vec![
        OsString::from("build"),
        OsString::from("--manifest-path"),
        unrelated_manifest.canonicalize().unwrap().into_os_string(),
    ];

    assert_eq!(args_with_profile(&args, &engine, &["mp3"]), args);
}

#[test]
fn custom_game_cargo_features_are_preserved() {
    let project = tempfile::tempdir().unwrap();
    let engine = project.path().join("engine");
    let manifest = engine.join("native/linux/Cargo.toml");
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    fs::write(&manifest, "[package]\nname = \"engine\"\n").unwrap();
    let args = vec![
        OsString::from("build"),
        OsString::from("--manifest-path"),
        manifest.canonicalize().unwrap().into_os_string(),
        OsString::from("--features"),
        OsString::from("debug-ui"),
    ];

    assert_eq!(args_with_profile(&args, &engine, &["mp3"]), args);
}

#[cfg(unix)]
#[test]
fn cargo_profile_proxy_forwards_selected_features_to_cargo() {
    let project = tempfile::tempdir().unwrap();
    let engine = project.path().join("node_modules/@bornengine/engine");
    let manifest = engine.join("native/linux/Cargo.toml");
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    fs::write(&manifest, "[package]\nname = \"engine\"\n").unwrap();

    let fake_cargo = project.path().join("fake-cargo");
    let output = project.path().join("cargo-args.txt");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$BORNENGINE_TEST_CARGO_ARGS\"\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&fake_cargo).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_cargo, permissions).unwrap();

    let result = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest.canonicalize().unwrap())
        .env(
            "BORNENGINE_CARGO_PROXY_ENGINE_ROOT",
            engine.canonicalize().unwrap(),
        )
        .env("BORNENGINE_CARGO_PROXY_REAL", fake_cargo)
        .env("BORNENGINE_CARGO_PROXY_FEATURES", "mp3")
        .env("BORNENGINE_TEST_CARGO_ARGS", &output)
        .output()
        .unwrap();

    assert!(result.status.success());
    let forwarded = fs::read_to_string(output).unwrap();
    assert!(
        forwarded
            .lines()
            .any(|line| line == "--no-default-features")
    );
    assert!(forwarded.lines().any(|line| line == "--features"));
    assert!(forwarded.lines().any(|line| line == "mp3"));
}
