# BornEngine CLI Tiled Import and Asset Packaging

For agentic workers: REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development`. Work in the clean CLI worktree on branch `work/tiled-assets`. The canonical target schema and fixture live in BornEngine `docs/superpowers/plans/2026-09-28-bornengine-world2d-runtime.md` and `tests/fixtures/world2d/`; consume the committed v1 contract, do not invent a second shape.

## Goal

Extend the existing `bornengine-cli` with a reliable Tiled TMX/TSX importer, asset validation, deterministic asset packing, and build/dev integration.

## Architecture

Use current Clap command routing and project/build helpers. Parse XML TMX/TSX through a bounded, non-networking parser. Normalize map/tileset paths against the project root, write canonical `bornengine.world2d` v1 JSON, and report diagnostics with file/layer/object/field context. Asset validation and packing share the same path-normalization/reference rules. Packing sorts normalized paths, writes a stable JSON manifest and copies exact bytes without timestamps or host-specific metadata.

## Tech Stack

Rust stable; existing Clap/serde/serde_json and test conventions; add minimal XML and SHA-256 dependencies only when needed; no external runtime service.

## Spec

Add `bornengine import tiled <map.tmx> --output <out.world2d.json>`, `bornengine assets validate [project-root]`, and `bornengine assets pack [project-root] --output <dir>`. The output must be the engine's `bornengine.world2d` v1 document: top-level `{format,version,id,name,assets,tilesets,layers,metadata}`; tile cells are `null` or `{tilesetId,tileId,flipX,flipY,flipDiagonal}` with zero-based local tile IDs; object layers retain object arrays, pixel transforms, typed property records, and component descriptors. `assets` is a sorted array of normalized relative POSIX paths; `tileset.tiles[]` retains zero-based ID, optional pixel-local rectangle collision, and typed properties. Support orthogonal maps; external TSX and image tilesets; XML tile data and CSV tile data; horizontal/vertical/diagonal flip flags; rectangle collision objects; layer/object/tile properties of string, int, float, bool, color, and file types. Unsupported compressed/base64 encodings, unsupported shapes/orientations, object-reference properties, and nested features return contextual errors instead of silently losing information. Build and run pack referenced assets into their managed output; watch mode includes referenced assets in its existing rebuild/restart path.

## Global Constraints

Do not create another CLI binary or change existing commands' semantics. Never follow symlinks outside the project root or fetch remote assets. Preserve Tiled coordinate order and typed properties. Deterministic output must not include absolute paths, timestamps, hash-map iteration order, or machine-specific separators. Keep the CLI repository history separate from BornEngine.

## Review Focus

Tiled GID flags decode correctly at the `firstgid` boundaries; TSX and images resolve relative to the file that references them; unsupported features never partially overwrite output; path traversal/symlink escapes fail; identical inputs create identical manifests and packed bytes; clean generated projects build/run with packaged assets.

---

### Task 1: Import, validate, pack, and integrate into CLI build workflow

**Files:** Modify `Cargo.toml`, `Cargo.lock`, `src/cli.rs`, `src/commands/mod.rs`, `src/commands/build.rs`, `src/build_artifacts.rs`, and `tests/cli_contract.rs` as needed. Add `src/commands/import.rs`, `src/commands/assets.rs`, `tests/tiled_import_contract.rs`, `tests/asset_pack_contract.rs`, and minimized fixtures under `tests/fixtures/tiled/` plus a byte-identical output fixture under `tests/fixtures/world2d/`.

1. Add failing Clap contract tests for command names/options and importer fixtures before implementation. Cover orthogonal XML/CSV, external TSX/image paths, Tiled flip flag combinations, tile/object typed properties, object collision rectangles, unsupported compression/orientation, actionable diagnostics, and safe no-output-on-error.
2. Add asset tests for missing references, case mismatch, traversal, escaping symlinks, deterministic sorted manifests, stable SHA-256 values, exact copy bytes, and rerunning on unchanged inputs. Test build output cleanup includes only managed pack files.
3. Implement one importer pipeline that resolves `firstgid` to `(tilesetId, tileId)` and strips Tiled flip bits into `WorldTileCell` booleans. Preserve layers, visibility, opacity, offsets, parallax, properties, and object transforms. Validate the output against the engine's canonical world2d validator fixture.
4. Implement `assets validate` and `assets pack`. Walk only project-owned asset roots and references; normalize all manifest paths to `/`; sort assets bytewise; emit no host-only metadata; reject symlink escape and duplicate normalized paths.
5. Integrate packing with `build`, `run`, and `dev --watch`. Track the generated manifest and copied assets in the CLI's managed build artifact record so `clean` removes them safely. Ensure watch changes to referenced files trigger the existing restart path.
6. Update CLI docs, `--help` tests, and a generated-project integration fixture. Run `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, `cargo test --locked --all-targets --all-features`, and `cargo build --locked --release`. Commit `feat: import tiled maps and package assets`.
