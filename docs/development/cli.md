# Developing the BornEngine CLI

The CLI is a Rust application with a small npm launcher. The Rust executable owns command parsing, project changes, builds, asset workflows, and diagnostics. The npm package locates and verifies the matching prebuilt executable before launching it.

## Repository map

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | Process entry point, Cargo proxy handoff, error reporting, exit code |
| `src/cli.rs` | Clap command and option definitions, parsing, and argument validation |
| `src/commands/mod.rs` | Dispatch from parsed commands to command implementations |
| `src/commands/` | Command behavior grouped by project, build, engine, cache, assets, import, configuration, and diagnostics |
| `src/project.rs` | Project metadata, project root discovery, and scaffold operations |
| `src/build_artifacts.rs` | Tracking build outputs so `clean` only removes files owned by the CLI |
| `src/cargo_profile.rs` | BornEngine native feature profiles and Cargo build environment |
| `src/engine.rs`, `src/engine_package.rs` | Engine dependency selection, release lookup, and package details |
| `src/platform.rs`, `src/process.rs` | Perry target discovery and child process invocation |
| `src/config.rs`, `src/package_manager.rs`, `src/ui.rs` | User defaults, package manager behavior, and terminal output |
| `tests/` | CLI contracts, command behavior, integration fixtures, and golden data |
| `npm/bin/bornengine.js`, `npm/lib/launcher.cjs` | npm executable shim and native binary download/cache/verification |
| `.github/workflows/ci.yml` | Cross-platform formatting, lint, Rust tests/build, and npm launcher checks |
| `.github/workflows/release.yml`, `.github/workflows/npm-release.yml` | Native release assets and npm package publication |

## Adding or changing a command

1. Define the command, options, aliases, and value parsers in `src/cli.rs`.
2. Route the parsed variant in `src/commands/mod.rs` to the appropriate command module. Keep command dispatch there; put behavior in the owning `src/commands/*.rs` file.
3. Move reusable logic into the appropriate shared module rather than making one command reach into another command's private implementation.
4. Add tests at the level of the contract being changed. Parser and help behavior belong in CLI contract tests; file generation, safety, and build behavior should use focused tests with temporary projects or fixtures.
5. Update `README.md` when the user-facing command list, flags, generated files, or behavior changes.

The npm launcher is not a second command implementation. Change `npm/bin/` or `npm/lib/` only when launcher behavior, platform asset selection, verification, caching, or npm packaging changes. Add or update its Node tests under `npm/test/` for those changes.

## Behavioral contracts to preserve

- `new` and `init` must not overwrite existing project files. Keep tests for refusing populated destinations and preserving files.
- `clean` must remove only files recorded as CLI-managed build artifacts. It must not delete dependencies, untracked files, or unrelated project output.
- Build target support is based on targets advertised by the installed Perry. Do not silently replace an unsupported requested target with another one.
- The CLI reads the BornEngine profile from the game project's `perry.toml` and forwards the corresponding Cargo features for `build`, `run`, and `dev`.
- Engine and CLI versions are released independently. Changes to one repository do not publish the other package.
- Keep generated paths project-relative and validate path traversal and symlink boundaries in asset and import workflows.

These contracts are described for users in the repository's root `README.md`; update that guide alongside any intentional behavior change.

## Local development

The Rust crate requires Rust 1.85 or newer, with stable Rust used by CI. The npm launcher package requires Node.js 18 or newer. From the CLI repository root:

```sh
cargo run -- --help
cargo run -- new --help
```

When validating a CLI change that integrates with engine code, use a game project pointed at a local BornEngine checkout with `--engine-path` (or `BORNENGINE_PATH`). Keep the engine repository and CLI repository changes independent, and run checks in both repositories for cross-repository behavior changes.

## Verification

These are the checks run by `.github/workflows/ci.yml`. Run them from the CLI repository root:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release
(cd npm && npm test)
(cd npm && npm pack --dry-run)
```

The Rust workflow runs on Linux, Windows, and macOS. The npm tests cover the launcher; `npm pack --dry-run` checks which files the npm package will include without publishing it.

## Release path

The CLI release workflow builds standalone binaries for Linux x86-64, Windows x86-64, macOS x86-64, and macOS ARM64 from a `v*` tag or an explicitly selected existing tag. It creates a GitHub release with those assets and a SHA-256 manifest. The npm release workflow publishes the launcher package after a GitHub release exists; the launcher downloads the matching native asset and verifies it before caching and starting it.

Review both workflows when changing executable names, target triples, archive names, checksums, npm files, or the download URL contract. The CLI and engine have separate versions and release flows.
