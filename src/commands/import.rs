use anyhow::{Context, Result, anyhow, bail};
use roxmltree::{Document, Node};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_XML_BYTES: u64 = 64 * 1024 * 1024;
const HORIZONTAL_FLIP: u32 = 0x8000_0000;
const VERTICAL_FLIP: u32 = 0x4000_0000;
const DIAGONAL_FLIP: u32 = 0x2000_0000;
const ROTATE_HEXAGONAL: u32 = 0x1000_0000;
const FLIP_MASK: u32 = HORIZONTAL_FLIP | VERTICAL_FLIP | DIAGONAL_FLIP;
const GID_MASK: u32 = !(FLIP_MASK | ROTATE_HEXAGONAL);

#[derive(Serialize)]
struct WorldDocument {
    format: &'static str,
    version: u32,
    id: String,
    name: String,
    assets: Vec<String>,
    tilesets: Vec<WorldTileset>,
    layers: Vec<WorldLayer>,
    metadata: BTreeMap<String, Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorldTileset {
    id: String,
    image: String,
    tile_width: u32,
    tile_height: u32,
    columns: u32,
    tile_count: u32,
    margin: PointU32,
    spacing: PointU32,
    tiles: Vec<WorldTileDefinition>,
}

#[derive(Serialize)]
struct PointU32 {
    x: u32,
    y: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorldTileDefinition {
    tile_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    collision: Option<Rect>,
    properties: BTreeMap<String, WorldProperty>,
}

#[derive(Serialize)]
struct Rect {
    x: Value,
    y: Value,
    width: Value,
    height: Value,
}

#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
enum WorldLayer {
    Tilemap {
        id: String,
        name: String,
        visible: bool,
        opacity: f64,
        offset: PointF64,
        parallax: PointF64,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        properties: BTreeMap<String, WorldProperty>,
        width: u32,
        height: u32,
        tile_size: PointU32,
        data: Vec<Option<WorldTileCell>>,
    },
    Objects {
        id: String,
        name: String,
        visible: bool,
        opacity: f64,
        offset: PointF64,
        parallax: PointF64,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        properties: BTreeMap<String, WorldProperty>,
        objects: Vec<WorldObject>,
    },
}

#[derive(Serialize)]
struct PointF64 {
    x: f64,
    y: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorldTileCell {
    tileset_id: String,
    tile_id: u32,
    flip_x: bool,
    flip_y: bool,
    flip_diagonal: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorldObject {
    id: String,
    name: String,
    #[serde(rename = "type")]
    object_type: String,
    position: PointF64,
    rotation: f64,
    size: PointF64,
    origin: PointF64,
    visible: bool,
    tags: Vec<String>,
    properties: BTreeMap<String, WorldProperty>,
    components: Vec<WorldComponent>,
}

#[derive(Serialize)]
struct WorldComponent {
    kind: &'static str,
    data: Value,
}

#[derive(Serialize)]
struct WorldProperty {
    #[serde(rename = "type")]
    property_type: &'static str,
    value: Value,
}

struct ParsedTileset {
    world: WorldTileset,
    first_gid: u32,
}

struct TileLayerContext<'a> {
    path: &'a Path,
    map_width: u32,
    map_height: u32,
    tile_width: u32,
    tile_height: u32,
    tilesets: &'a [ParsedTileset],
    root: &'a Path,
    assets: &'a mut BTreeSet<String>,
    index: u32,
}

/// Convert an orthogonal TMX map to the versioned BornEngine world format.
///
/// Paths in the output are relative to `project_root`; referenced files are
/// resolved relative to the TMX or TSX file that names them.
pub fn import_tiled(map_file: &Path, output: &Path, project_root: &Path) -> Result<()> {
    let output_name = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("output path has no valid file name: {}", output.display()))?;
    if !output_name.ends_with(".world2d.json") {
        bail!(
            "output file must end with `.world2d.json`: {}",
            output.display()
        );
    }
    let root = project_root
        .canonicalize()
        .with_context(|| format!("project root does not exist: {}", project_root.display()))?;
    if !root.is_dir() {
        bail!("project root is not a directory: {}", root.display());
    }
    let map_path = canonical_file(map_file, "Tiled map")?;
    ensure_inside_root(&root, &map_path, "Tiled map")?;
    let map_text = read_xml(&map_path)?;
    let map_doc = parse_xml(&map_text, &map_path)?;
    let map = map_doc.root_element();
    require_tag(map, "map", &map_path)?;

    if required_attr(map, "orientation", &map_path)? != "orthogonal" {
        bail!(
            "{}: only orthogonal Tiled maps are supported",
            map_path.display()
        );
    }
    if bool_attr(map, "infinite", false, &map_path)? {
        bail!(
            "{}: infinite Tiled maps are not supported; export a finite orthogonal map",
            map_path.display()
        );
    }
    if map
        .attribute("class")
        .is_some_and(|value| !value.is_empty())
    {
        bail!(
            "{}: Tiled map classes are not supported",
            map_path.display()
        );
    }
    if map
        .attribute("renderorder")
        .is_some_and(|value| value != "right-down")
    {
        bail!(
            "{}: only Tiled `right-down` render order is supported",
            map_path.display()
        );
    }
    let map_width = u32_attr(map, "width", &map_path)?;
    let map_height = u32_attr(map, "height", &map_path)?;
    let map_tile_width = u32_attr(map, "tilewidth", &map_path)?;
    let map_tile_height = u32_attr(map, "tileheight", &map_path)?;
    if map_width == 0 || map_height == 0 || map_tile_width == 0 || map_tile_height == 0 {
        bail!(
            "{}: map dimensions and tile dimensions must be positive",
            map_path.display()
        );
    }

    let mut assets = BTreeSet::new();
    let mut parsed_tilesets = Vec::new();
    let mut used_ids = BTreeSet::new();
    for reference in map.children().filter(|node| node.has_tag_name("tileset")) {
        let first_gid = u32_attr(reference, "firstgid", &map_path)?;
        if first_gid == 0 {
            bail!(
                "{}: tileset firstgid must be greater than zero",
                map_path.display()
            );
        }
        let (tileset_node, source_path, source_text) =
            if let Some(source) = reference.attribute("source") {
                let source_path = resolve_file_reference(&root, &map_path, source, "external TSX")?;
                let source_text = read_xml(&source_path)?;
                let source_doc = parse_xml(&source_text, &source_path)?;
                let node = source_doc.root_element();
                require_tag(node, "tileset", &source_path)?;
                let parsed = parse_tileset_node(node, &source_path, &map_path, &root, &mut assets)?;
                parsed_tilesets.push(ParsedTileset {
                    world: parsed,
                    first_gid,
                });
                continue;
            } else {
                (reference, map_path.as_path(), String::new())
            };
        let _ = source_text;
        let world = parse_tileset_node(tileset_node, source_path, &map_path, &root, &mut assets)?;
        parsed_tilesets.push(ParsedTileset { world, first_gid });
    }
    parsed_tilesets.sort_by_key(|tileset| tileset.first_gid);
    for tileset in &mut parsed_tilesets {
        if !used_ids.insert(tileset.world.id.clone()) {
            let base = tileset.world.id.clone();
            let mut suffix = 2_u32;
            while !used_ids.insert(format!("{base}-{suffix}")) {
                suffix = suffix.saturating_add(1);
            }
            tileset.world.id = format!("{base}-{suffix}");
        }
    }
    if parsed_tilesets.is_empty() {
        bail!(
            "{}: map does not reference any tilesets",
            map_path.display()
        );
    }

    let mut layers = Vec::new();
    let mut layer_index = 0_u32;
    for child in map.children().filter(|node| node.is_element()) {
        if child.has_tag_name("tileset") || child.has_tag_name("properties") {
            continue;
        }
        layer_index += 1;
        if child.has_tag_name("group") {
            bail!(
                "{}: nested Tiled groups are not supported",
                map_path.display()
            );
        }
        if child.has_tag_name("layer") {
            layers.push(parse_tile_layer(
                child,
                TileLayerContext {
                    path: &map_path,
                    map_width,
                    map_height,
                    tile_width: map_tile_width,
                    tile_height: map_tile_height,
                    tilesets: &parsed_tilesets,
                    root: &root,
                    assets: &mut assets,
                    index: layer_index,
                },
            )?);
        } else if child.has_tag_name("objectgroup") {
            layers.push(parse_object_layer(
                child,
                &map_path,
                &root,
                &mut assets,
                layer_index,
            )?);
        } else {
            bail!(
                "{}: unsupported map element <{}>",
                map_path.display(),
                child.tag_name().name()
            );
        }
    }

    let map_properties = parse_properties(map, &map_path, &map_path, &root, &mut assets)?;
    let metadata = map_properties
        .into_iter()
        .map(|(name, property)| (name, serde_json::to_value(property).unwrap_or(Value::Null)))
        .collect();
    let map_stem = map_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("world");
    let document = WorldDocument {
        format: "bornengine.world2d",
        version: 1,
        id: map_stem.to_owned(),
        name: humanize_name(map_stem),
        assets: assets.into_iter().collect(),
        tilesets: parsed_tilesets
            .into_iter()
            .map(|tileset| tileset.world)
            .collect(),
        layers,
        metadata,
    };
    let mut bytes = serde_json::to_vec_pretty(&document)?;
    bytes.push(b'\n');
    atomic_write(output, &bytes)
}

fn parse_tileset_node(
    node: Node<'_, '_>,
    source_path: &Path,
    map_path: &Path,
    root: &Path,
    assets: &mut BTreeSet<String>,
) -> Result<WorldTileset> {
    if node
        .attribute("class")
        .is_some_and(|value| !value.is_empty())
    {
        bail!(
            "{}: Tiled tileset classes are not supported",
            source_path.display()
        );
    }
    if let Some(offset) = child(node, "tileoffset") {
        if f64_attr_or(offset, "x", 0.0, source_path)? != 0.0
            || f64_attr_or(offset, "y", 0.0, source_path)? != 0.0
        {
            bail!(
                "{}: nonzero Tiled tile offsets cannot be represented by World2D v1",
                source_path.display()
            );
        }
    }
    let name = node
        .attribute("name")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| source_path.file_stem().and_then(|value| value.to_str()))
        .unwrap_or("tileset");
    let image_node = child(node, "image").ok_or_else(|| {
        anyhow!(
            "{}: image-collection tilesets are not supported; use a single atlas image",
            source_path.display()
        )
    })?;
    let image_source = required_attr(image_node, "source", source_path)?;
    let image_file = resolve_file_reference(root, source_path, image_source, "tileset image")?;
    let image_path = relative_project_path(root, &image_file)?;
    assets.insert(image_path.clone());

    let tile_width = u32_attr(node, "tilewidth", source_path)?;
    let tile_height = u32_attr(node, "tileheight", source_path)?;
    let margin = u32_attr_or(node, "margin", 0, source_path)?;
    let spacing = u32_attr_or(node, "spacing", 0, source_path)?;
    let image_width = image_node
        .attribute("width")
        .map(|value| parse_u32(value, source_path, "image width"))
        .transpose()?;
    let image_height = image_node
        .attribute("height")
        .map(|value| parse_u32(value, source_path, "image height"))
        .transpose()?;
    let columns = u32_attr_or(node, "columns", 0, source_path)?;
    let columns = if columns > 0 {
        columns
    } else if let Some(width) = image_width {
        infer_grid_count(width, tile_width, margin, spacing).with_context(|| {
            format!("{}: could not infer tileset columns", source_path.display())
        })?
    } else {
        bail!(
            "{}: tileset needs a positive `columns` value or image width",
            source_path.display()
        );
    };
    let tile_count = if let Some(count) = node.attribute("tilecount") {
        parse_u32(count, source_path, "tilecount")?
    } else if let Some(height) = image_height {
        let rows = infer_grid_count(height, tile_height, margin, spacing)
            .with_context(|| format!("{}: could not infer tileset rows", source_path.display()))?;
        columns
            .checked_mul(rows)
            .ok_or_else(|| anyhow!("{}: tileset tilecount overflows", source_path.display()))?
    } else {
        bail!(
            "{}: tileset needs `tilecount` or image dimensions",
            source_path.display()
        );
    };
    if tile_width == 0 || tile_height == 0 || columns == 0 || tile_count == 0 {
        bail!(
            "{}: tileset dimensions, columns, and tilecount must be positive",
            source_path.display()
        );
    }

    let mut tiles = Vec::new();
    let mut seen_tile_ids = BTreeSet::new();
    for tile in node.children().filter(|node| node.has_tag_name("tile")) {
        let tile_id = u32_attr(tile, "id", source_path)?;
        if tile_id >= tile_count {
            bail!(
                "{}: tile id {tile_id} is outside tilecount {tile_count}",
                source_path.display()
            );
        }
        if !seen_tile_ids.insert(tile_id) {
            bail!("{}: duplicate tile id {tile_id}", source_path.display());
        }
        if tile
            .attribute("class")
            .or_else(|| tile.attribute("type"))
            .is_some_and(|value| !value.is_empty())
        {
            bail!(
                "{}: Tiled per-tile classes are not supported (tile {tile_id})",
                source_path.display()
            );
        }
        if child(tile, "animation").is_some() {
            bail!(
                "{}: Tiled tile animations are not supported (tile {tile_id})",
                source_path.display()
            );
        }
        if child(tile, "image").is_some() {
            bail!(
                "{}: image-collection tiles are not supported (tile {tile_id})",
                source_path.display()
            );
        }
        let properties = parse_properties(tile, source_path, map_path, root, assets)?;
        let collision = parse_tile_collision(tile, source_path)?;
        if collision.is_some() || !properties.is_empty() {
            tiles.push(WorldTileDefinition {
                tile_id,
                collision,
                properties,
            });
        }
    }
    tiles.sort_by_key(|tile| tile.tile_id);
    Ok(WorldTileset {
        id: name.to_owned(),
        image: image_path,
        tile_width,
        tile_height,
        columns,
        tile_count,
        margin: PointU32 {
            x: margin,
            y: margin,
        },
        spacing: PointU32 {
            x: spacing,
            y: spacing,
        },
        tiles,
    })
}

fn parse_tile_collision(tile: Node<'_, '_>, path: &Path) -> Result<Option<Rect>> {
    let Some(group) = child(tile, "objectgroup") else {
        return Ok(None);
    };
    let objects = group
        .children()
        .filter(|node| node.has_tag_name("object"))
        .collect::<Vec<_>>();
    if objects.len() > 1 {
        bail!(
            "{}: tile collision supports a single rectangle per tile",
            path.display()
        );
    }
    let Some(object) = objects.first().copied() else {
        return Ok(None);
    };
    reject_non_rect_shape(object, path)?;
    let rotation = f64_attr_or(object, "rotation", 0.0, path)?;
    if rotation != 0.0 {
        bail!(
            "{}: rotated tile collision rectangles are not supported",
            path.display()
        );
    }
    let width = f64_attr_or(object, "width", 0.0, path)?;
    let height = f64_attr_or(object, "height", 0.0, path)?;
    if width <= 0.0 || height <= 0.0 {
        bail!(
            "{}: tile collision rectangle must have positive width and height",
            path.display()
        );
    }
    Ok(Some(Rect {
        x: numeric_attr_or(object, "x", "0", path)?,
        y: numeric_attr_or(object, "y", "0", path)?,
        width: numeric_attr_or(object, "width", "0", path)?,
        height: numeric_attr_or(object, "height", "0", path)?,
    }))
}

fn parse_tile_layer(layer: Node<'_, '_>, context: TileLayerContext<'_>) -> Result<WorldLayer> {
    let TileLayerContext {
        path,
        map_width,
        map_height,
        tile_width,
        tile_height,
        tilesets,
        root,
        assets,
        index,
    } = context;
    let name = layer.attribute("name").unwrap_or("Layer").to_owned();
    if layer
        .attribute("class")
        .is_some_and(|value| !value.is_empty())
    {
        bail!(
            "{}: layer `{name}` has a Tiled class that World2D v1 cannot represent",
            path.display()
        );
    }
    if f64_attr_or(layer, "x", 0.0, path)? != 0.0 || f64_attr_or(layer, "y", 0.0, path)? != 0.0 {
        bail!(
            "{}: layer `{name}` uses a tile offset that World2D v1 cannot represent",
            path.display()
        );
    }
    if layer.attribute("tintcolor").is_some()
        || layer
            .attribute("mode")
            .is_some_and(|value| value != "normal")
    {
        bail!(
            "{}: layer `{name}` uses tint or blending settings that World2D v1 cannot represent",
            path.display()
        );
    }
    let width = u32_attr_or(layer, "width", map_width, path)?;
    let height = u32_attr_or(layer, "height", map_height, path)?;
    if width != map_width || height != map_height {
        bail!(
            "{}: layer `{name}` dimensions must match the finite map dimensions",
            path.display()
        );
    }
    let data = child(layer, "data")
        .ok_or_else(|| anyhow!("{}: layer `{name}` has no data", path.display()))?;
    if data.attribute("compression").is_some() && data.attribute("encoding") == Some("base64") {
        bail!(
            "{}: layer `{name}` uses compressed base64 tile data, which is not supported",
            path.display()
        );
    }
    if data.attribute("compression").is_some() {
        bail!(
            "{}: layer `{name}` uses compressed tile data, which is not supported",
            path.display()
        );
    }
    let encoding = data.attribute("encoding").unwrap_or("xml");
    let raw_gids = match encoding {
        "csv" => data
            .text()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| parse_u32(value, path, &format!("layer `{name}` CSV gid")))
            .collect::<Result<Vec<_>>>()?,
        "xml" => data
            .children()
            .filter(|node| node.has_tag_name("tile"))
            .map(|tile| u32_attr(tile, "gid", path))
            .collect::<Result<Vec<_>>>()?,
        "base64" => bail!(
            "{}: layer `{name}` uses base64 tile data, which is not supported",
            path.display()
        ),
        other => bail!(
            "{}: layer `{name}` uses unsupported encoding `{other}`",
            path.display()
        ),
    };
    let expected_count = width as usize * height as usize;
    if raw_gids.len() != expected_count {
        bail!(
            "{}: layer `{name}` contains {} cells, expected {expected_count}",
            path.display(),
            raw_gids.len()
        );
    }
    let mut cells = Vec::with_capacity(raw_gids.len());
    for (cell_index, raw_gid) in raw_gids.into_iter().enumerate() {
        if raw_gid & ROTATE_HEXAGONAL != 0 {
            bail!(
                "{}: layer `{name}` cell {cell_index} contains the hexagonal rotation flag on an orthogonal map",
                path.display()
            );
        }
        let gid = raw_gid & GID_MASK;
        if gid == 0 {
            cells.push(None);
            continue;
        }
        let tileset = tilesets.iter().rev().find(|tileset| gid >= tileset.first_gid)
            .ok_or_else(|| anyhow!("{}: layer `{name}` cell {cell_index} references gid {gid} before any tileset firstgid", path.display()))?;
        let tile_id = gid - tileset.first_gid;
        if tile_id >= tileset.world.tile_count {
            bail!(
                "{}: layer `{name}` cell {cell_index} gid {gid} falls outside tileset `{}`",
                path.display(),
                tileset.world.id
            );
        }
        cells.push(Some(WorldTileCell {
            tileset_id: tileset.world.id.clone(),
            tile_id,
            flip_x: raw_gid & HORIZONTAL_FLIP != 0,
            flip_y: raw_gid & VERTICAL_FLIP != 0,
            flip_diagonal: raw_gid & DIAGONAL_FLIP != 0,
        }));
    }
    let opacity = f64_attr_or(layer, "opacity", 1.0, path)?;
    if !(0.0..=1.0).contains(&opacity) {
        bail!(
            "{}: layer `{name}` opacity must be between 0 and 1",
            path.display()
        );
    }
    Ok(WorldLayer::Tilemap {
        id: layer_id(layer, index),
        name,
        visible: bool_attr(layer, "visible", true, path)?,
        opacity,
        offset: PointF64 {
            x: f64_attr_or(layer, "offsetx", 0.0, path)?,
            y: f64_attr_or(layer, "offsety", 0.0, path)?,
        },
        parallax: PointF64 {
            x: f64_attr_or(layer, "parallaxx", 1.0, path)?,
            y: f64_attr_or(layer, "parallaxy", 1.0, path)?,
        },
        properties: parse_properties(layer, path, path, root, assets)?,
        width,
        height,
        tile_size: PointU32 {
            x: tile_width,
            y: tile_height,
        },
        data: cells,
    })
}

fn parse_object_layer(
    layer: Node<'_, '_>,
    path: &Path,
    root: &Path,
    assets: &mut BTreeSet<String>,
    index: u32,
) -> Result<WorldLayer> {
    let name = layer.attribute("name").unwrap_or("Objects");
    if layer
        .attribute("class")
        .is_some_and(|value| !value.is_empty())
    {
        bail!(
            "{}: object layer `{name}` has a Tiled class that World2D v1 cannot represent",
            path.display()
        );
    }
    if f64_attr_or(layer, "x", 0.0, path)? != 0.0 || f64_attr_or(layer, "y", 0.0, path)? != 0.0 {
        bail!(
            "{}: object layer `{name}` uses a tile offset that World2D v1 cannot represent",
            path.display()
        );
    }
    if layer.attribute("tintcolor").is_some()
        || layer
            .attribute("mode")
            .is_some_and(|value| value != "normal")
    {
        bail!(
            "{}: object layer `{name}` uses tint or blending settings that World2D v1 cannot represent",
            path.display()
        );
    }
    let mut objects = Vec::new();
    for object in layer.children().filter(|node| node.has_tag_name("object")) {
        reject_non_rect_shape(object, path)?;
        if object.attribute("gid").is_some() {
            bail!(
                "{}: Tiled tile objects are not supported yet",
                path.display()
            );
        }
        if object.attribute("template").is_some() {
            bail!(
                "{}: Tiled object templates are not supported",
                path.display()
            );
        }
        let name = object.attribute("name").unwrap_or("").to_owned();
        let object_type = object
            .attribute("class")
            .or_else(|| object.attribute("type"))
            .unwrap_or("")
            .to_owned();
        let width = f64_attr_or(object, "width", 0.0, path)?;
        let height = f64_attr_or(object, "height", 0.0, path)?;
        if (width == 0.0) != (height == 0.0) || width < 0.0 || height < 0.0 {
            bail!(
                "{}: object `{name}` must be a point or a positive-size rectangle",
                path.display()
            );
        }
        let rotation = f64_attr_or(object, "rotation", 0.0, path)?;
        let object_opacity = f64_attr_or(object, "opacity", 1.0, path)?;
        if object_opacity != 1.0 {
            bail!(
                "{}: object `{name}` opacity cannot be represented by World2D v1",
                path.display()
            );
        }
        let properties = parse_properties(object, path, path, root, assets)?;
        let components = if object_type == "collision" && width > 0.0 && height > 0.0 {
            vec![WorldComponent {
                kind: "physicsBody2D",
                data: serde_json::json!({
                    "type": "static",
                    "shape": { "type": "box", "width": width, "height": height },
                    "isSensor": false,
                    "layer": 1,
                    "mask": 2147483647_u32
                }),
            }]
        } else {
            Vec::new()
        };
        objects.push(WorldObject {
            id: object
                .attribute("id")
                .map(|id| format!("object-{id}"))
                .unwrap_or_else(|| format!("object-{}", objects.len() + 1)),
            name,
            object_type,
            position: PointF64 {
                x: f64_attr_or(object, "x", 0.0, path)?,
                y: f64_attr_or(object, "y", 0.0, path)?,
            },
            rotation,
            size: PointF64 {
                x: width,
                y: height,
            },
            origin: PointF64 { x: 0.0, y: 0.0 },
            visible: bool_attr(object, "visible", true, path)?,
            tags: Vec::new(),
            properties,
            components,
        });
    }
    let properties = parse_properties(layer, path, path, root, assets)?;
    let opacity = f64_attr_or(layer, "opacity", 1.0, path)?;
    if !(0.0..=1.0).contains(&opacity) {
        bail!(
            "{}: object layer `{name}` opacity must be between 0 and 1",
            path.display()
        );
    }
    Ok(WorldLayer::Objects {
        id: layer_id(layer, index),
        name: name.to_owned(),
        visible: bool_attr(layer, "visible", true, path)?,
        opacity,
        offset: PointF64 {
            x: f64_attr_or(layer, "offsetx", 0.0, path)?,
            y: f64_attr_or(layer, "offsety", 0.0, path)?,
        },
        parallax: PointF64 {
            x: f64_attr_or(layer, "parallaxx", 1.0, path)?,
            y: f64_attr_or(layer, "parallaxy", 1.0, path)?,
        },
        properties,
        objects,
    })
}

fn parse_properties(
    owner: Node<'_, '_>,
    context_path: &Path,
    property_base_file: &Path,
    root: &Path,
    assets: &mut BTreeSet<String>,
) -> Result<BTreeMap<String, WorldProperty>> {
    let mut properties = BTreeMap::new();
    let Some(container) = child(owner, "properties") else {
        return Ok(properties);
    };
    if container
        .attribute("class")
        .is_some_and(|value| !value.is_empty())
    {
        bail!(
            "{}: Tiled class properties are not supported",
            context_path.display()
        );
    }
    for property in container
        .children()
        .filter(|node| node.has_tag_name("property"))
    {
        let name = required_attr(property, "name", context_path)?.to_owned();
        let kind = property.attribute("type").unwrap_or("string");
        let raw_value = property
            .attribute("value")
            .or_else(|| property.text())
            .unwrap_or("");
        let text = raw_value.trim();
        let (property_type, value) = match kind {
            "string" => ("string", Value::String(raw_value.to_owned())),
            "int" => (
                "int",
                Value::Number(
                    text.parse::<i64>()
                        .with_context(|| {
                            format!(
                                "{}: property `{name}` is not a valid integer",
                                context_path.display()
                            )
                        })?
                        .into(),
                ),
            ),
            "float" => (
                "float",
                finite_number(
                    text.parse::<f64>().with_context(|| {
                        format!(
                            "{}: property `{name}` is not a valid float",
                            context_path.display()
                        )
                    })?,
                    context_path,
                    &name,
                )?,
            ),
            "bool" => ("bool", Value::Bool(parse_bool(text, context_path, &name)?)),
            "color" => ("color", Value::String(text.to_owned())),
            "file" => {
                let resolved = resolve_file_reference(
                    root,
                    property_base_file,
                    text,
                    &format!("file property `{name}`"),
                )?;
                let relative = relative_project_path(root, &resolved)?;
                assets.insert(relative.clone());
                ("file", Value::String(relative))
            }
            "object" => bail!(
                "{}: object-reference property `{name}` is not supported",
                context_path.display()
            ),
            other => bail!(
                "{}: property `{name}` has unsupported type `{other}`",
                context_path.display()
            ),
        };
        if properties
            .insert(
                name.clone(),
                WorldProperty {
                    property_type,
                    value,
                },
            )
            .is_some()
        {
            bail!("{}: duplicate property `{name}`", context_path.display());
        }
    }
    Ok(properties)
}

fn reject_non_rect_shape(object: Node<'_, '_>, path: &Path) -> Result<()> {
    for child in object.children().filter(|node| node.is_element()) {
        if matches!(
            child.tag_name().name(),
            "ellipse" | "polygon" | "polyline" | "text" | "capsule"
        ) {
            bail!(
                "{}: Tiled object shape `<{}>` is not supported; use rectangles or point objects",
                path.display(),
                child.tag_name().name()
            );
        }
    }
    Ok(())
}

fn resolve_file_reference(
    root: &Path,
    referring_file: &Path,
    value: &str,
    label: &str,
) -> Result<PathBuf> {
    let value = value.trim();
    if value.is_empty() || value.contains("://") {
        bail!(
            "{}: {label} path must be a local project file",
            referring_file.display()
        );
    }
    let source = Path::new(value);
    if source.is_absolute() {
        bail!(
            "{}: {label} path must be relative: {value}",
            referring_file.display()
        );
    }
    let resolved = canonical_file(&referring_file.parent().unwrap_or(root).join(source), label)?;
    ensure_inside_root(root, &resolved, label)?;
    Ok(resolved)
}

fn relative_project_path(root: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(root).with_context(|| {
        format!(
            "{} is outside project root {}",
            path.display(),
            root.display()
        )
    })?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("asset path is unsafe: {}", relative.display());
    }
    let normalized = relative.to_string_lossy().replace('\\', "/");
    if normalized.is_empty() || normalized.starts_with('/') {
        bail!("asset path is unsafe: {normalized}");
    }
    Ok(normalized)
}

fn canonical_file(path: &Path, label: &str) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("{label} does not exist: {}", path.display()))?;
    if !canonical.is_file() {
        bail!("{label} is not a file: {}", path.display());
    }
    Ok(canonical)
}

fn ensure_inside_root(root: &Path, path: &Path, label: &str) -> Result<()> {
    if !path.starts_with(root) {
        bail!("{label} resolves outside project root: {}", path.display());
    }
    Ok(())
}

fn read_xml(path: &Path) -> Result<String> {
    let metadata =
        fs::metadata(path).with_context(|| format!("could not inspect {}", path.display()))?;
    if metadata.len() > MAX_XML_BYTES {
        bail!("{} exceeds the 64 MiB XML import limit", path.display());
    }
    fs::read_to_string(path).with_context(|| format!("could not read UTF-8 XML {}", path.display()))
}

fn parse_xml<'a>(contents: &'a str, path: &Path) -> Result<Document<'a>> {
    Document::parse(contents).map_err(|error| anyhow!("{}: invalid XML: {error}", path.display()))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("could not create output directory {}", parent.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("output path has no file name: {}", path.display()))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    fs::write(&temporary, bytes)
        .with_context(|| format!("could not write temporary output {}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("could not replace output {}", path.display()));
    }
    Ok(())
}

fn infer_grid_count(pixels: u32, tile_pixels: u32, margin: u32, spacing: u32) -> Result<u32> {
    if tile_pixels == 0 {
        bail!("tile dimension must be positive");
    }
    let total_margin = margin
        .checked_mul(2)
        .ok_or_else(|| anyhow!("tileset margin overflows"))?;
    let usable = pixels
        .checked_sub(total_margin)
        .ok_or_else(|| anyhow!("margin exceeds image size"))?;
    let step = tile_pixels
        .checked_add(spacing)
        .ok_or_else(|| anyhow!("tile spacing overflows"))?;
    let extent = usable
        .checked_add(spacing)
        .ok_or_else(|| anyhow!("tileset image dimensions overflow"))?;
    if step == 0 || extent % step != 0 {
        bail!("image dimensions do not align with tile size, margin, and spacing");
    }
    Ok(extent / step)
}

fn layer_id(node: Node<'_, '_>, index: u32) -> String {
    node.attribute("id")
        .map(|id| format!("layer-{id}"))
        .unwrap_or_else(|| format!("layer-{index}"))
}

fn humanize_name(value: &str) -> String {
    let separated = value.replace(['-', '_'], " ");
    let mut chars = separated.chars();
    match chars.next() {
        None => "World".to_owned(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|child| child.has_tag_name(name))
}

fn require_tag(node: Node<'_, '_>, name: &str, path: &Path) -> Result<()> {
    if !node.has_tag_name(name) {
        bail!("{}: expected <{name}> root element", path.display());
    }
    Ok(())
}

fn required_attr<'a>(node: Node<'a, '_>, name: &str, path: &Path) -> Result<&'a str> {
    node.attribute(name).with_context(|| {
        format!(
            "{}: missing `{name}` attribute on <{}>",
            path.display(),
            node.tag_name().name()
        )
    })
}

fn u32_attr(node: Node<'_, '_>, name: &str, path: &Path) -> Result<u32> {
    parse_u32(required_attr(node, name, path)?, path, name)
}

fn u32_attr_or(node: Node<'_, '_>, name: &str, default: u32, path: &Path) -> Result<u32> {
    node.attribute(name)
        .map(|value| parse_u32(value, path, name))
        .unwrap_or(Ok(default))
}

fn parse_u32(value: &str, path: &Path, field: &str) -> Result<u32> {
    value
        .trim()
        .parse::<u32>()
        .with_context(|| format!("{}: `{field}` must be an unsigned integer", path.display()))
}

fn f64_attr_or(node: Node<'_, '_>, name: &str, default: f64, path: &Path) -> Result<f64> {
    match node.attribute(name) {
        Some(value) => finite_f64(
            value
                .trim()
                .parse::<f64>()
                .with_context(|| format!("{}: `{name}` must be numeric", path.display()))?,
            path,
            name,
        ),
        None => Ok(default),
    }
}

fn finite_number(value: f64, path: &Path, field: &str) -> Result<Value> {
    let value = finite_f64(value, path, field)?;
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| anyhow!("{}: `{field}` is not a JSON number", path.display()))
}

fn finite_f64(value: f64, path: &Path, field: &str) -> Result<f64> {
    if !value.is_finite() {
        bail!("{}: `{field}` must be finite", path.display());
    }
    Ok(value)
}

fn numeric_attr_or(node: Node<'_, '_>, name: &str, default: &str, path: &Path) -> Result<Value> {
    let raw = node.attribute(name).unwrap_or(default).trim();
    if raw.contains(['.', 'e', 'E']) {
        return finite_number(
            raw.parse::<f64>()
                .with_context(|| format!("{}: `{name}` must be numeric", path.display()))?,
            path,
            name,
        );
    }
    if let Ok(value) = raw.parse::<i64>() {
        return Ok(Value::Number(value.into()));
    }
    finite_number(
        raw.parse::<f64>()
            .with_context(|| format!("{}: `{name}` must be numeric", path.display()))?,
        path,
        name,
    )
}

fn bool_attr(node: Node<'_, '_>, name: &str, default: bool, path: &Path) -> Result<bool> {
    node.attribute(name)
        .map(|value| parse_bool(value, path, name))
        .unwrap_or(Ok(default))
}

fn parse_bool(value: &str, path: &Path, field: &str) -> Result<bool> {
    match value.trim() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => bail!("{}: `{field}` must be true or false", path.display()),
    }
}
