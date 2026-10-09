# BornEngine CLI

`bornengine` creates, builds, runs, and manages BornEngine game projects. It is a small Rust frontend to Perry: Perry compiles TypeScript, while each game records its own engine dependency and lockfile.

> **Lineage:** This is a standalone companion CLI for [BornEngine](https://github.com/RuanFernandes/BornEngine), an independently maintained fork of the original [Bloom Engine](https://github.com/Bloom-Engine/engine). Bloom Engine remains the upstream project; this CLI is not an official Bloom Engine tool and is not affiliated with or endorsed by its maintainers.

## Install

Install the CLI from npm:

```sh
npm install --global @bornengine/cli
```

The npm launcher downloads the matching native binary on first use and caches it for later runs. To install the Rust binary directly instead, use Cargo:

```sh
cargo install --git https://github.com/RuanFernandes/bornengine-cli
```

The downloaded native binary does not require Node.js; the npm launcher requires Node.js 18 or newer. Game projects need Perry and a package manager; new projects default to `pnpm` and can also use `npm` or `yarn`.

## Quick start

```sh
bornengine create # Enter MyGame when prompted
cd MyGame
bornengine run main.ts
```

`create` interactively asks for a project name, game profile, package manager, and stable engine version from npm. It uses your configured package manager and engine version as the initial selections. For scripts and non-interactive use, `new` creates a starter game, writes the AI guide as `AGENTS.md`, writes Perry's native-library allowlist and BornEngine's Rust profile to `perry.toml`, installs the selected engine package, and creates the package manager lockfile. Use the BornEngine CLI's `build`, `run`, or `dev` command so it can apply the selected Rust profile. Normal projects do not require a separate BornEngine clone.

## Commands

| Command | Purpose |
| --- | --- |
| `bornengine create` | Interactively select a project name, 2D/2.5D/3D profile, package manager, and stable engine version from npm. |
| `bornengine new <name>` | Create a project with a 2D/2.5D/3D native feature profile and install its dependencies. |
| `bornengine --add-ai-docs <filename>` | Write the BornEngine AI guide to `<filename>.md` in the current directory without overwriting files. |
| `bornengine init` | Add a profile-configured project scaffold to an otherwise empty directory. It refuses to overwrite generated files. |
| `bornengine build <entry>` | Compile for the host or a requested target. |
| `bornengine run <entry>` | Build and run for the current host. |
| `bornengine dev <entry>` | Build and run once; pass `--watch` to use Perry's rebuild-and-restart loop. |
| `bornengine cache path` | Show the shared Cargo build cache used for native builds. |
| `bornengine cache warm` | Precompile this project's native engine into the shared cache. |
| `bornengine check <entry>` | Run Perry's source compatibility check, optionally including installed-package analysis. |
| `bornengine import tiled <map.tmx> --output <world.world2d.json>` | Convert an orthogonal Tiled map to BornEngine's versioned world format. |
| `bornengine assets validate [project-root]` | Validate packaged asset roots and `.world2d.json` references. |
| `bornengine assets pack [project-root] --output <directory>` | Copy project assets and write a deterministic SHA-256 manifest. |
| `bornengine clean` | Remove only build files recorded by this CLI. |
| `bornengine perry install [--release <tag>]` | Download the Perry compiler for this host from a BornEngine GitHub release (latest by default), verify its SHA-256, and use it for builds. |
| `bornengine perry path` | Show the Perry compiler builds will run. |
| `bornengine perry list` | List Perry compilers installed by `perry install`; the active one is marked `(current)`. |
| `bornengine perry clean [--dry-run]` | Remove installed Perry compilers other than the current one, plus interrupted downloads. Use `--dry-run` to list them first. |
| `bornengine doctor` | Check Perry, Rust, the package manager, the project, and host prerequisites. |
| `bornengine info` / `version` | Show CLI, engine, Perry, project, and host details. |
| `bornengine engine current` | Show the project's selected engine dependency. |
| `bornengine engine install [version]` | Select a stable engine release and install it. |
| `bornengine engine list` | List stable engine releases available from npm. |
| `bornengine engine update` / `upgrade` | Update the project engine dependency without changing other dependencies. |
| `bornengine engine use <version-or-path>` | Select a stable version or local checkout. |
| `bornengine engine remove [version]` | Remove the project engine dependency. |
| `bornengine update` | Check for a CLI release and print an install command; it never self-updates. |
| `bornengine config set|get|list` | Configure the default package manager and engine version. |

Examples:

```sh
bornengine new MyGame --game-type 2d --package-manager npm --engine-version 0.15.0
bornengine --add-ai-docs game-context
bornengine new MyScriptedGame --native-features sqlite,scripting
bornengine new MyAdventure --game-type 2.5d
bornengine new MyWorld --game-type 3d
bornengine init --game-type 3d
bornengine build main.ts --name my-game --os linux
bornengine build main.ts --target ios-simulator
bornengine dev main.ts --watch
bornengine run main.ts --jobs 4
bornengine run main.ts --release
bornengine cache path
bornengine cache warm --jobs 4
bornengine import tiled maps/level.tmx --output worlds/level.world2d.json
bornengine assets validate
bornengine assets pack --output dist/game
bornengine config set package-manager pnpm
```

`--game-type` accepts `2d`, `2.5d`, or `3d` and defaults to `2d`; `--kind` is an alias. It stores the profile in `[bornengine].native_profile` in `perry.toml`. During `bornengine build`, `run`, and `dev`, the CLI forwards the selected Cargo features to BornEngine's native Rust crate; `dev` also enables hot reload. It does not change builds of other Cargo packages. The 2D profile enables MP3 decoding and omits Jolt physics, 3D model loading, and extra image codecs; 2.5D adds model loading and common 3D image formats; 3D also enables Jolt.

SQLite and embedded scripting are opt-in native features. Pass `--native-features sqlite,scripting` to `new` or `init`, or add those names to `[bornengine].native_features` in an existing project's `perry.toml`. The default project does not compile the optional native SQLite or QuickJS dependencies. `sqlite` enables native database support; `scripting` enables QuickJS where that runtime is supported. Web/WASM keeps its existing behavior and uses its prebuilt engine package. Other custom engine Cargo features, such as `debug-ui`, can be set directly in `native_features`. Direct `perry compile` commands do not read the BornEngine CLI profile.

## Native build cache and development profiles

The first native build for a particular engine version, Rust toolchain, target, and feature combination still compiles the Rust dependencies. The CLI streams compiler output and stores compatible Cargo artifacts in a shared per-user cache, normally `<user-cache>/BornEngine/cargo-target`; later projects can reuse those artifacts. Check the active path or override it with `CARGO_TARGET_DIR`:

```sh
bornengine cache path
CARGO_TARGET_DIR=/mnt/fast-cache bornengine run main.ts
```

Use `bornengine cache warm` inside an installed BornEngine project to compile the selected native engine/profile before compiling the TypeScript game. It does not build or launch the game. `bornengine run` and `bornengine dev` use a faster incremental native profile by default; add `--release` for optimized output. `bornengine build` stays optimized. Add `--jobs N` to `build`, `run`, `dev`, or `cache warm` to set Cargo's parallel job count. When omitted, the CLI respects inherited `CARGO_BUILD_JOBS` and Cargo's normal scheduling. Installing the CLI does not compile engine artifacts.

The package manager can be shortened to `--pm`; `--engine` aliases `--engine-path`. `-o` is the friendly OS selector, and `-n` / `--name` sets the output name. `--os` and `--target` are mutually exclusive.

Scaffolding never deletes or overwrites existing files. `new` refuses a populated target directory, and `init` creates only files that do not already exist. `--add-ai-docs` also refuses to overwrite its destination and appends `.md` when the supplied filename does not already end with that extension. There is intentionally no `--force` option.

## Engine dependency and local development

Each game pins an exact stable engine version in `package.json`; its lockfile records the resolved graph. New projects and engine installs prefer the published `@bornengine/engine` package. The CLI still recognizes `@bloomengine/engine` for existing Bloom-based projects and uses it as a fallback when a requested version is unavailable under the BornEngine scope. The CLI does not install a machine-global engine.

For engine development, point a game at a local checkout:

```sh
git clone https://github.com/RuanFernandes/BornEngine
bornengine new EngineTest --engine-path ../BornEngine
```

Or use `BORNENGINE_PATH=/path/to/BornEngine` as a default override. An explicit `--engine-path` takes precedence. `pnpm` uses a local `link:` dependency; `npm` and `yarn` use `file:` dependencies. The local checkout's package name determines the generated imports and Perry allowlist.

To switch an existing game between a local checkout and a release:

```sh
bornengine engine use ../BornEngine
bornengine engine use 0.4.17
```

## Targets and output

Without `--os` or `--target`, the CLI uses Perry's native host target. Friendly `--os` values include `linux`, `windows`, `macos`, `android`, `ios`, `web`, `tvos`, `watchos`, and `visionos`; an OS is accepted only when the installed Perry advertises the corresponding target. `--target` passes an exact target advertised by the installed Perry, including newer or more specific targets unknown to this CLI.

Cross-compilation is available only when the installed Perry and its platform toolchain support the selected target. The CLI does not silently substitute another platform. macOS uses Perry's native macOS target and therefore needs a macOS host. `run` only accepts native executables that match the current host; web and mobile builds are build-only. Web/WASM outputs use Perry's HTML output format.

Build outputs are isolated under `.bornengine/builds/`. Watch-mode output uses `.perry-dev/`, which Perry excludes from its source watcher. Both paths are ignored by the generated project's Git configuration. `clean` removes only files recorded in the CLI manifest; it does not delete dependencies or untracked files.

## Perry compatibility checks

`bornengine check main.ts --check-deps` asks the installed Perry compiler to inspect dependencies in `node_modules` used by the entry point. Add `--deep-deps` to scan the full installed dependency tree instead of only direct imports. Perry owns the compatibility analysis and its diagnostics are shown as reported; the CLI does not infer that an untested package is incompatible. Perry warnings are findings to review, not proof that a package cannot run. Use `--all` to include all Perry findings, including hints, and `--strict` to make Perry treat its warnings as errors:

```sh
bornengine check main.ts --check-deps
bornengine check main.ts --check-deps --deep-deps --all
bornengine check main.ts --check-deps --strict
```

The scan analyzes what Perry can observe in installed package code and declarations. A successful check is not a guarantee of runtime behavior for every code path or platform. `--deep-deps` requires `--check-deps`.

## Tiled maps and game assets

`import tiled` accepts finite orthogonal TMX maps with CSV or XML tile data, external TSX tilesets and atlas images. It preserves layer transforms and properties, typed string/integer/float/boolean/color/file values, tile GID flip flags, and rectangular object collisions in a `bornengine.world2d` v1 document. Tileset images resolve relative to their TSX file; Tiled file-property paths resolve relative to the map. All referenced files must stay inside the current project root. The importer reports unsupported features with the map or TSX path and does not replace the output if conversion fails.

Isometric or infinite maps, base64/compressed tile data, image-collection tilesets, nested groups, tile objects, non-rectangular collision shapes, and object-reference properties are not imported. Export a finite orthogonal map and use a single atlas tileset image when using this workflow.

`assets validate` and `assets pack` inspect the project's `assets/`, `public/`, and `static/` roots plus assets listed by BornEngine world documents. They reject missing references, unsafe paths, case mismatches, and symlinks that escape the project. Packing preserves project-relative paths, copies exact file bytes, sorts manifest entries, and includes each file's SHA-256 digest and size. `build` and `run` place the pack beside the managed executable; `dev --watch` also asks Perry to watch the asset directories so a changed image or data file restarts the game. `clean` removes the packed files only when they are recorded under a CLI-managed build directory.

### Asset audit configuration

Place an optional `bornengine.assets.json` at the project root:

```json
{
  "version": 1,
  "dynamic_paths": ["assets/skins/player.png"],
  "ignored_paths": ["assets/generated/**"],
  "orphan_severity": "warning",
  "max_file_bytes": 10485760,
  "max_total_bytes": 104857600,
  "max_image_dimension": 4096,
  "max_total_image_pixels": 16777216
}
```

`version` must be `1`. `dynamic_paths` lists exact existing files loaded through computed runtime paths. Such paths cannot be inferred from TypeScript expressions, so declare them to avoid orphan warnings. A missing declaration target is an error. `ignored_paths` excludes matching files from orphan and size/pixel budget checks, but they remain in the validation inventory and asset pack; media header and extension diagnostics still apply. Paths use forward slashes relative to the project root. Absolute paths, traversal, duplicate entries, and unsupported glob syntax are rejected. In `ignored_paths`, `*` matches within one path component and `**` matches whole components across directories.

`orphan_severity` accepts `ignore`, `warn` (or `warning`), and `error`; the default is `warning`. The four limits are optional positive integers. `max_file_bytes` checks each nonignored file; `max_total_bytes` sums nonignored files. `max_image_dimension` checks either dimension of each nonignored supported image; `max_total_image_pixels` sums their width times height. Command flags `--orphan-policy {ignore,warn,error}`, `--max-file-bytes`, `--max-total-bytes`, `--max-image-dimension`, and `--max-total-image-pixels` override the matching manifest value. Unspecified flags leave manifest values in place. There is no implicit size or dimension limit.

The auditor recognizes PNG, JPEG (`.jpg`/`.jpeg`), GIF, BMP, WebP, WAV, MP3, Ogg, and FLAC signatures. It reads headers and image dimensions without decoding whole media files. Invalid headers and signature/extension mismatches are warnings. Reference analysis uses supported `.world2d.json` declarations and `dynamic_paths`; it cannot prove use of arbitrary TypeScript asset paths. An orphan warning means no known reference was found, not that the file is unused at runtime.

### Validation output and exit status

Human output retains `Validated N project assets (B bytes)` on stdout and prints each diagnostic to stderr with its severity, stable code, project-relative path, and message. `--json` writes one JSON object followed by a newline to stdout, without human summary or diagnostic text. Its schema is:

```json
{
  "format": "bornengine.asset_validation",
  "version": 1,
  "summary": { "files": 1, "bytes": 6 },
  "diagnostics": [{ "code": "orphan_asset", "severity": "warning", "path": "assets/example.txt", "message": "asset has no known static or declared dynamic reference", "measured": null, "limit": null }]
}
```

Diagnostics are sorted by path, code, severity, message, measured value, and limit. Codes are `orphan_asset`, `invalid_media_header`, `extension_mismatch`, `file_size_limit`, `total_size_limit`, `image_dimension_limit`, `total_image_pixels_limit`, and `declared_dynamic_path_missing`. Aggregate diagnostics use an empty path. Measured and limit values are bytes, pixels, or the maximum image dimension as appropriate; unrelated diagnostics use `null`. A report with warnings only exits `0`; any error diagnostic exits `1` in both output modes. Invalid manifests, unsafe paths, missing world references, and other hard validation failures also exit `1` and print an error to stderr; they cannot produce a completed JSON report.

`assets pack` continues to copy the same deterministic file inventory and write a `bornengine-assets-v1` manifest. Audit warnings do not change the default pack output. Build and run still fail on hard validation errors or configured error diagnostics before packing.

For the browsable command reference and configuration guide, see [Asset audit in the BornEngine documentation](https://ruanfernandes.github.io/BornEngine/docs/cli/assets/).

## Troubleshooting

- **Perry not found:** run `bornengine perry install`, or make `perry` available on `PATH`, then run `bornengine doctor`. `BORNENGINE_PERRY` selects a specific binary.
- **Package manager missing:** install the selected manager. For the default, install Node.js and run `npm install --global pnpm`.
- **Linux game prerequisites:** a native Linux engine build needs `pkg-config`, X11/XI headers, and ALSA headers. On Debian/Ubuntu: `sudo apt install pkg-config libx11-dev libxi-dev libasound2-dev`.
- **A Perry runtime archive is missing:** Perry's native linker needs its target runtime library. Follow the diagnostic from Perry to install/build that matching runtime; the CLI does not update Perry automatically.
- **A target is rejected:** inspect `perry compile --help` on that machine. Only targets advertised by that installed compiler are accepted.
- **Dependency installation failed:** the generated source files are retained. From the project directory, retry with the selected manager's `install` command.

## Releases

The CLI and engine have separate versions and release workflows. A CLI tag such as `v0.5.0` produces standalone release assets and a SHA-256 manifest for Linux x86-64, Windows x86-64, macOS x86-64, and macOS ARM64. The published `@bornengine/cli` package downloads and verifies the matching asset on first use. Alternatively, install from GitHub with Cargo as shown above.

## License

MIT. See [LICENSE](LICENSE).
