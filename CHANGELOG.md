# Changelog

Notable changes to BornEngine CLI are documented here. CLI releases are versioned independently from the BornEngine engine.

## Unreleased

### Added

- Native feature auto-detection: `multiplayer` and `dialogs` are enabled from the project's imports and file-dialog calls. Set `[bornengine].auto_native_features = false` to opt out.
- `bornengine create server` scaffolds a linked Colyseus server in `server/` (or another empty child folder) and writes `bornengine.server.json`.
- Initial Rust CLI for scaffolding, building, running, checking, and managing BornEngine projects.
- Perry target discovery, environment diagnostics, project-local engine versions, and safe build cleanup.
- Independent CI and multi-platform GitHub release workflows.
