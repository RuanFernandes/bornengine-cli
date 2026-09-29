use bornengine_cli::commands::import::import_tiled;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;

#[test]
fn imports_csv_orthogonal_map_external_tileset_flips_properties_and_collision_rectangles() {
    let fixture_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiled/csv-external");
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join("tiny.world2d.json");

    import_tiled(&fixture_root.join("maps/tiny.tmx"), &output, &fixture_root).unwrap();

    let actual: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    let expected: Value = serde_json::from_slice(
        &fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/world2d/tiled-golden.world2d.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn resolves_multiple_firstgid_boundaries_to_local_tileset_ids() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("maps")).unwrap();
    fs::create_dir_all(project.path().join("tiles")).unwrap();
    fs::create_dir_all(project.path().join("assets")).unwrap();
    for name in ["stone", "water"] {
        fs::write(project.path().join(format!("assets/{name}.png")), [1]).unwrap();
        fs::write(
            project.path().join(format!("tiles/{name}.tsx")),
            format!(
                "<tileset name=\"{name}\" tilewidth=\"8\" tileheight=\"8\" tilecount=\"1\" columns=\"1\"><image source=\"../assets/{name}.png\" width=\"8\" height=\"8\"/></tileset>"
            ),
        )
        .unwrap();
    }
    fs::write(
        project.path().join("maps/boundary.tmx"),
        r#"<map orientation="orthogonal" width="2" height="1" tilewidth="8" tileheight="8"><tileset firstgid="1" source="../tiles/stone.tsx"/><tileset firstgid="2" source="../tiles/water.tsx"/><layer id="1" name="ground" width="2" height="1"><data encoding="csv">1,2</data></layer></map>"#,
    )
    .unwrap();
    let output = project.path().join("worlds/boundary.world2d.json");

    import_tiled(
        &project.path().join("maps/boundary.tmx"),
        &output,
        project.path(),
    )
    .unwrap();

    let world: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert_eq!(world["layers"][0]["data"][0]["tilesetId"], "stone");
    assert_eq!(world["layers"][0]["data"][0]["tileId"], 0);
    assert_eq!(world["layers"][0]["data"][1]["tilesetId"], "water");
    assert_eq!(world["layers"][0]["data"][1]["tileId"], 0);
}

#[test]
fn imports_unencoded_xml_tile_data() {
    let fixture_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiled/xml-inline");
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join("inline.world2d.json");

    import_tiled(&fixture_root.join("inline.tmx"), &output, &fixture_root).unwrap();

    let actual: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(actual["layers"][0]["data"][0]["tileId"], 0);
}

#[test]
fn preserves_integer_color_and_file_properties_and_tracks_file_assets() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("maps")).unwrap();
    fs::create_dir_all(project.path().join("tilesets/sub")).unwrap();
    fs::create_dir_all(project.path().join("assets")).unwrap();
    fs::write(project.path().join("assets/sheet.png"), [0, 1]).unwrap();
    fs::write(project.path().join("assets/tile-ref.dat"), [2, 3]).unwrap();
    fs::write(project.path().join("assets/object-ref.dat"), [4, 5]).unwrap();
    fs::write(
        project.path().join("tilesets/sub/terrain.tsx"),
        r##"<tileset name="terrain" tilewidth="8" tileheight="8" tilecount="1" columns="1"><image source="../../assets/sheet.png" width="8" height="8"/><tile id="0"><properties><property name="weight" type="int" value="7"/><property name="palette" type="color" value="#ffff0000"/><property name="sidecar" type="file" value="../assets/tile-ref.dat"/></properties></tile></tileset>"##,
    )
    .unwrap();
    fs::write(
        project.path().join("maps/level.tmx"),
        r#"<map orientation="orthogonal" width="1" height="1" tilewidth="8" tileheight="8"><tileset firstgid="1" source="../tilesets/sub/terrain.tsx"/><layer id="1" name="ground" width="1" height="1"><data encoding="csv">1</data><properties><property name="surface" value="grass"/></properties></layer><objectgroup id="2" name="objects"><properties><property name="encounter" type="bool" value="true"/></properties><object id="1" name="spawn" type="spawn" x="4" y="4"><properties><property name="order" type="int" value="2"/><property name="portrait" type="file" value="../assets/object-ref.dat"/><property name="caption" type="string" value=" padded text "/></properties></object></objectgroup></map>"#,
    )
    .unwrap();
    let output = project.path().join("worlds/level.world2d.json");

    import_tiled(
        &project.path().join("maps/level.tmx"),
        &output,
        project.path(),
    )
    .unwrap();

    let world: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert_eq!(
        world["assets"],
        serde_json::json!([
            "assets/object-ref.dat",
            "assets/sheet.png",
            "assets/tile-ref.dat"
        ])
    );
    assert_eq!(
        world["tilesets"][0]["tiles"][0]["properties"]["weight"]["type"],
        "int"
    );
    assert_eq!(
        world["tilesets"][0]["tiles"][0]["properties"]["weight"]["value"],
        7
    );
    assert_eq!(
        world["tilesets"][0]["tiles"][0]["properties"]["palette"]["type"],
        "color"
    );
    assert_eq!(
        world["tilesets"][0]["tiles"][0]["properties"]["sidecar"]["value"],
        "assets/tile-ref.dat"
    );
    assert_eq!(
        world["layers"][1]["objects"][0]["properties"]["order"]["value"],
        2
    );
    assert_eq!(
        world["layers"][1]["objects"][0]["properties"]["portrait"]["value"],
        "assets/object-ref.dat"
    );
    assert_eq!(
        world["layers"][1]["objects"][0]["properties"]["caption"]["value"],
        " padded text "
    );
    assert_eq!(
        world["layers"][0]["properties"]["surface"]["value"],
        "grass"
    );
    assert_eq!(world["layers"][1]["properties"]["encounter"]["value"], true);
}

#[test]
fn rejects_compressed_base64_with_file_context_without_replacing_output() {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tiled/unsupported-compression");
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join("previous.world2d.json");
    fs::write(&output, b"previous valid world").unwrap();

    let error = import_tiled(&fixture_root.join("compressed.tmx"), &output, &fixture_root)
        .unwrap_err()
        .to_string();

    assert!(error.contains("compressed.tmx"), "{error}");
    assert!(error.contains("base64"), "{error}");
    assert_eq!(fs::read(&output).unwrap(), b"previous valid world");
}

#[test]
fn rejects_empty_csv_cells_instead_of_silently_dropping_them() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("maps")).unwrap();
    fs::create_dir_all(project.path().join("tiles")).unwrap();
    fs::create_dir_all(project.path().join("assets")).unwrap();
    fs::write(project.path().join("assets/sheet.png"), [1]).unwrap();
    fs::write(
        project.path().join("tiles/terrain.tsx"),
        r#"<tileset name="terrain" tilewidth="8" tileheight="8" tilecount="1" columns="1"><image source="../assets/sheet.png" width="8" height="8"/></tileset>"#,
    )
    .unwrap();
    fs::write(
        project.path().join("maps/empty-cell.tmx"),
        r#"<map orientation="orthogonal" width="1" height="1" tilewidth="8" tileheight="8"><tileset firstgid="1" source="../tiles/terrain.tsx"/><layer id="1" name="ground" width="1" height="1"><data encoding="csv">1,</data></layer></map>"#,
    )
    .unwrap();
    let output = project.path().join("worlds/empty-cell.world2d.json");

    let error = import_tiled(
        &project.path().join("maps/empty-cell.tmx"),
        &output,
        project.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("empty-cell.tmx"), "{error}");
    assert!(error.contains("empty CSV cell"), "{error}");
    assert!(!output.exists());
}

#[cfg(unix)]
#[test]
fn rejects_tiled_asset_symlink_escape_without_writing_world() {
    use std::os::unix::fs::symlink;

    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("maps")).unwrap();
    fs::create_dir_all(project.path().join("tiles")).unwrap();
    fs::create_dir_all(project.path().join("assets")).unwrap();
    fs::write(external.path().join("outside.png"), [1, 2, 3]).unwrap();
    symlink(
        external.path().join("outside.png"),
        project.path().join("assets/outside.png"),
    )
    .unwrap();
    fs::write(
        project.path().join("tiles/terrain.tsx"),
        r#"<tileset name="terrain" tilewidth="8" tileheight="8" tilecount="1" columns="1"><image source="../assets/outside.png" width="8" height="8"/></tileset>"#,
    )
    .unwrap();
    fs::write(
        project.path().join("maps/level.tmx"),
        r#"<map orientation="orthogonal" width="1" height="1" tilewidth="8" tileheight="8"><tileset firstgid="1" source="../tiles/terrain.tsx"/><layer id="1" name="ground" width="1" height="1"><data encoding="csv">1</data></layer></map>"#,
    )
    .unwrap();
    let output = project.path().join("worlds/level.world2d.json");

    let error = import_tiled(
        &project.path().join("maps/level.tmx"),
        &output,
        project.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("outside") && error.contains("project root"),
        "{error}"
    );
    assert!(error.contains("outside.png"), "{error}");
    assert!(!output.exists());
}

#[test]
fn rejects_isometric_map_with_actionable_orientation_diagnostic() {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tiled/unsupported-orientation");
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join("must-not-exist.world2d.json");

    let error = import_tiled(&fixture_root.join("iso.tmx"), &output, &fixture_root)
        .unwrap_err()
        .to_string();

    assert!(error.contains("iso.tmx"), "{error}");
    assert!(error.contains("orthogonal"), "{error}");
    assert!(!output.exists());
}

#[test]
fn rejects_unsupported_object_shapes_without_writing_partial_world() {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tiled/unsupported-shapes");
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join("shape.world2d.json");

    let error = import_tiled(&fixture_root.join("ellipse.tmx"), &output, &fixture_root)
        .unwrap_err()
        .to_string();

    assert!(error.contains("ellipse.tmx"), "{error}");
    assert!(error.contains("shape"), "{error}");
    assert!(error.contains("rectangles or point objects"), "{error}");
    assert!(!output.exists());
}

#[test]
fn rejects_object_reference_properties_without_writing_partial_world() {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tiled/unsupported-object-property");
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join("object-ref.world2d.json");

    let error = import_tiled(
        &fixture_root.join("object-reference.tmx"),
        &output,
        &fixture_root,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("object-reference.tmx"), "{error}");
    assert!(error.contains("target"), "{error}");
    assert!(error.contains("object"), "{error}");
    assert!(!output.exists());
}

#[test]
fn canonical_tiled_output_fixture_matches_the_engine_repository_contract() {
    let fixture = fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/world2d/tiled-golden.world2d.json"),
    )
    .unwrap();
    let sha256 = Sha256::digest(&fixture)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    assert_eq!(
        sha256,
        "3bb2570c0ceaf34469ac35ae5e0b86c41a71ad7b4ce9abecc239cc4bd62618c6"
    );
    let world: Value = serde_json::from_slice(&fixture).unwrap();
    assert_eq!(world["format"], "bornengine.world2d");
    assert_eq!(world["version"], 1);
}
