use serde_json::Value;
use std::fs;
use std::process::Command;

#[test]
fn generated_project_can_import_validate_and_pack_a_tiled_level() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{"name":"tiny-game","private":true}"#,
    )
    .unwrap();

    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiled/csv-external");
    for directory in ["maps", "tilesets", "assets"] {
        fs::create_dir_all(project.path().join(directory)).unwrap();
    }
    fs::copy(
        fixtures.join("maps/tiny.tmx"),
        project.path().join("maps/tiny.tmx"),
    )
    .unwrap();
    fs::copy(
        fixtures.join("tilesets/terrain.tsx"),
        project.path().join("tilesets/terrain.tsx"),
    )
    .unwrap();
    fs::copy(
        fixtures.join("assets/terrain.png"),
        project.path().join("assets/terrain.png"),
    )
    .unwrap();

    let cli = env!("CARGO_BIN_EXE_bornengine");
    let imported = Command::new(cli)
        .current_dir(project.path())
        .args([
            "import",
            "tiled",
            "maps/tiny.tmx",
            "--output",
            "worlds/tiny.world2d.json",
        ])
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );

    let world: Value =
        serde_json::from_slice(&fs::read(project.path().join("worlds/tiny.world2d.json")).unwrap())
            .unwrap();
    assert_eq!(world["format"], "bornengine.world2d");
    assert_eq!(world["layers"][0]["data"][3]["flipDiagonal"], true);

    let validated = Command::new(cli)
        .current_dir(project.path())
        .args(["assets", "validate"])
        .output()
        .unwrap();
    assert!(
        validated.status.success(),
        "{}",
        String::from_utf8_lossy(&validated.stderr)
    );

    let packed = Command::new(cli)
        .current_dir(project.path())
        .args(["assets", "pack", "--output", "dist/game"])
        .output()
        .unwrap();
    assert!(
        packed.status.success(),
        "{}",
        String::from_utf8_lossy(&packed.stderr)
    );
    assert_eq!(
        fs::read(project.path().join("assets/terrain.png")).unwrap(),
        fs::read(project.path().join("dist/game/assets/terrain.png")).unwrap()
    );
    assert!(
        project
            .path()
            .join("dist/game/assets.manifest.json")
            .is_file()
    );
}
