# Changelog

Notable changes to BornEngine CLI are documented here. CLI releases are versioned independently from the BornEngine engine.

## 0.8.0

### Added

- Native feature auto-detection: `multiplayer` and `dialogs` are enabled from the project's imports and file-dialog calls. Set `[bornengine].auto_native_features = false` to opt out.
- `bornengine create server` scaffolds a linked Colyseus server in `server/` (or another empty child folder) and writes `bornengine.server.json`.
- Initial Rust CLI for scaffolding, building, running, checking, and managing BornEngine projects.
- Perry target discovery, environment diagnostics, project-local engine versions, and safe build cleanup.
- Independent CI and multi-platform GitHub release workflows.

### Fixed

- Asset packing adopts an existing output file whose bytes match the project asset, so assets copied next to a Perry binary no longer block `bornengine run`/`build`. Different untracked files are still refused.
- Asset validation, build, run, and dev accept World2D format version 2 documents written by the engine, collecting tileset images, file properties, and extra `assets` entries; version 1 is validated as before.
