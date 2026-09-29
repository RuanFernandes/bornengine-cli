# BornEngine CLI Asset Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` in the dedicated CLI worktree. The engine/CLI repositories remain separate and must end with matching integration branches and PRs.

**Goal:** Extend the existing `bornengine assets validate` command with deterministic orphan, media-header, and budget diagnostics while preserving safe asset packing.

**Architecture:** Add a versioned project manifest `bornengine.assets.json` and structured validation options/report. CLI flags override project settings. Known references come from World2D/Tiled data and explicit dynamic-path declarations; arbitrary TypeScript expressions are not inferred. Existing `assets pack`, `build`, `run`, and watch workflows keep using the same safe file inventory.

**Engine specification:** `BornEngine/docs/superpowers/specs/2026-09-29-bornengine-2d-professional-services-design.md`.

**Repository:** `bornengine-cli` at `main` v0.2.0, integrated with the already-approved `integrate/2d-production` CLI branch on a new `integrate/2d-professional` review branch.

## Setup and constraints

1. Create `integrate/2d-professional` from current CLI `main`; merge the prior CLI `integrate/2d-production` branch into this new branch and resolve conflicts here, leaving both source branches unchanged. Use the frozen subclass API from the BornEngine lifecycle workstream when updating the starter.
2. Run the existing CLI tests before feature edits and record the baseline.
3. Create `work/assets-audit` from the new integration commit. Keep pack format `bornengine-assets-v1` unless a demonstrated compatibility requirement makes a new version necessary.
4. Do not add a second CLI. Keep diagnostics path-safe, deterministic, stable-code based, and relative to the project root. Do not output terminal colors in JSON mode.

## Task 1: contract fixtures and project manifest

**Files:** Add `tests/asset_validation_report.rs`; extend `tests/asset_pack_contract.rs`; modify `src/commands/assets.rs`, `src/cli.rs`, `src/commands/mod.rs`, and `README.md`.

1. Add fixture cases for known references, unreferenced assets, explicit dynamic paths, exclusions, invalid asset manifest, malformed media headers, extension/signature mismatch, file and aggregate size limits, image dimensions/pixels, diagnostics sorting, JSON output, symlinks, and traversal attempts.
2. Add a versioned `bornengine.assets.json` parser with fields for version, dynamic paths, ignored paths, orphan severity, and optional file/aggregate/image budgets. Reject absolute paths, parent traversal, unsupported glob syntax, duplicate entries, invalid limits, and unsupported future versions. With no manifest or flags, preserve existing validation errors, exit behavior, and pack inventory; new audit findings are warnings only.
3. Define `AssetDiagnostic { code, severity, path, message, measured, limit }` and a deterministic `AssetValidationReport`; retain a default validation entry point for existing build/pack callers.
4. Make CLI flags override manifest settings. Add `--json`, orphan policy, maximum file bytes, maximum total bytes, maximum image dimension, and maximum aggregate image pixels. Missing limits remain unset; known orphans default to warnings and do not fail a build unless promoted to errors.
5. Verify diagnostics use normalized project-relative paths and stable code/severity values; equivalent projects on Windows and Unix produce the same JSON ordering and values.

## Task 2: media and orphan analysis

**Files:** Modify `src/commands/assets.rs`; add media-header helpers under `src/commands/assets/` only if separation improves testability.

1. Reuse the current canonical-root, symlink, World2D reference, asset-root, and path-normalization logic. Do not add a second file walker or weaken existing path safety.
2. Build the referenced set from supported `.world2d.json` declarations and the manifest's dynamic paths. Treat ignored globs as excluded from the orphan audit and documented budget scope; keep project configuration itself out of the pack inventory unless explicitly listed.
3. Classify each discovered image/audio file by signature and extension. Diagnose invalid or unreadable headers and mismatched extensions. Read image width/height from supported headers, without decoding or rewriting full media. Do not claim to know whether computed TypeScript paths are used.
4. Emit `orphan_asset`, `invalid_media_header`, `extension_mismatch`, `file_size_limit`, `total_size_limit`, `image_dimension_limit`, `total_image_pixels_limit`, and `declared_dynamic_path_missing` diagnostics in a stable order.
5. Preserve existing hard errors for missing references, unsafe paths, symlink escapes, and case mismatch. Advisory diagnostics return success; error-severity configured diagnostics fail `assets validate` and any build/run path that validates before packing.
6. Confirm package output and `bornengine-assets-v1` manifest bytes are unchanged for existing fixtures when no new budget is configured.

## Task 3: command integration, starter, and docs

**Files:** Modify `src/cli.rs`, `src/commands/mod.rs`, `src/project.rs`, and `README.md`; update CLI golden tests.

1. Connect flags/config to the structured report and print human diagnostics to stderr while keeping the existing summary on stdout. `--json` emits only one versioned JSON report on stdout.
2. Update the generated `main.ts` project starter from free-function `initWindow`/`runGame` to a minimal `Game` subclass using `onStart`, `loop`, and `render`.
3. Document manifest fields, flag precedence, exit-code rules, report schema, recognized media probes, advisory behavior, dynamic-path declarations, and limits of static reference analysis.
4. Keep `assets pack` deterministic and document that warnings do not change default pack output.

## Verification

Run from the CLI worktree:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release
```

Add CLI contract tests for the generated starter and prove the starter passes `bornengine assets validate` and Perry's strict compatibility check against the engine integration branch. Merge the completed worktree into `integrate/2d-professional`, review the diff, and open a CLI PR. Do not publish or merge to `main` before the user reviews both PRs.

**Commit:** `feat: expand asset validation diagnostics`
