# BornEngine CLI

`bornengine` creates, builds, runs, and manages BornEngine game projects. It is a small Rust frontend to Perry: Perry compiles TypeScript, while each game records its own engine dependency and lockfile.

> **Lineage:** This is a standalone companion CLI for [BornEngine](https://github.com/RuanFernandes/BornEngine), an independently maintained fork of the original [Bloom Engine](https://github.com/Bloom-Engine/engine). Bloom Engine remains the upstream project; this CLI is not an official Bloom Engine tool and is not affiliated with or endorsed by its maintainers.

## Install

Install the CLI from this repository:

```sh
cargo install --git https://github.com/RuanFernandes/bornengine-cli
```

The CLI itself does not require Node.js. Game projects need Perry and a package manager; new projects default to `pnpm` and can also use `npm` or `yarn`.

## Quick start

```sh
bornengine create # Enter MyGame when prompted
cd MyGame
bornengine run main.ts
```

`create` interactively asks for a project name, package manager, and stable engine version from npm. It uses your configured package manager and engine version as the initial selections. For scripts and non-interactive use, `new` creates a starter game, writes Perry's native-library allowlist, installs the selected engine package, and creates the package manager lockfile. Normal projects do not require a separate BornEngine clone.

## Commands

| Command | Purpose |
| --- | --- |
| `bornengine create` | Interactively select a project name, package manager, and stable engine version from npm. |
| `bornengine new <name>` | Create a project and install its dependencies. |
| `bornengine init` | Add a new project scaffold to an otherwise empty directory. It refuses to overwrite generated files. |
| `bornengine build <entry>` | Compile for the host or a requested target. |
| `bornengine run <entry>` | Build and run for the current host. |
| `bornengine dev <entry>` | Build and run once; pass `--watch` to use Perry's rebuild-and-restart loop. |
| `bornengine check <entry>` | Run Perry's compatibility check. |
| `bornengine import tiled <map.tmx> --output <world.world2d.json>` | Convert an orthogonal Tiled map to BornEngine's versioned world format. |
| `bornengine assets validate [project-root]` | Validate packaged asset roots and `.world2d.json` references. |
| `bornengine assets pack [project-root] --output <directory>` | Copy project assets and write a deterministic SHA-256 manifest. |
| `bornengine script check [--manifest <path>]` | Validate a self-contained JavaScript behavior package. |
| `bornengine script pack --output <directory> [--manifest <path>]` | Write the declared script entry and package manifest to a dedicated output directory. |
| `bornengine clean` | Remove only build files recorded by this CLI. |
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
bornengine new MyGame --package-manager npm --engine-version 0.4.17
bornengine build main.ts --name my-game --os linux
bornengine build main.ts --target ios-simulator
bornengine dev main.ts --watch
bornengine import tiled maps/level.tmx --output worlds/level.world2d.json
bornengine assets validate
bornengine assets pack --output dist/game
bornengine script check
bornengine script pack --output dist/scripts/player
bornengine config set package-manager pnpm
```

The package manager can be shortened to `--pm`; `--engine` aliases `--engine-path`. `-o` is the friendly OS selector, and `-n` / `--name` sets the output name. `--os` and `--target` are mutually exclusive.

Scaffolding never deletes or overwrites existing files. `new` refuses a populated target directory, and `init` creates only files that do not already exist. There is intentionally no `--force` option.

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

## Script behavior packages

Place a `bornengine.script.json` at the package root:

```json
{
  "format": "bornengine-script-v1",
  "apiVersion": 1,
  "entry": "scripts/player.js",
  "permissions": ["log", "self.read"]
}
```

The `entry` must be a regular UTF-8 `.js` or `.mjs` file inside the package, no larger than 1 MiB. Permissions must be sorted, unique, and selected from `log`, `self.particles.emit`, `self.read`, and `self.transform.write`. `script check` parses the module without executing JavaScript. It rejects source deeper than 128 delimiter levels or with more than 128 recursive syntax steps in an expression chain. Flat array and object entries remain allowed within the 1 MiB source limit. Static imports, re-exports from another module, and dynamic imports are rejected because v1 has no module loader. The input manifest, any previous packed manifest, and the pack ownership marker are each limited to 64 KiB. `script pack` applies the same checks, then writes the declared entry, manifest, and `.bornengine-pack.json` ownership marker. The marker records hashes of both package files. Use an empty output directory or one previously created by this command; a separately authored package, changed packaged file, symbolic link, or untracked content is never replaced. Repeated packs produce the same output bytes. The pack summary counts all three output files.

## Troubleshooting

- **Perry not found:** install Perry and make sure `perry` is on `PATH`, then run `bornengine doctor`.
- **Package manager missing:** install the selected manager. For the default, install Node.js and run `npm install --global pnpm`.
- **Linux game prerequisites:** a native Linux engine build needs `pkg-config`, X11/XI headers, and ALSA headers. On Debian/Ubuntu: `sudo apt install pkg-config libx11-dev libxi-dev libasound2-dev`.
- **A Perry runtime archive is missing:** Perry's native linker needs its target runtime library. Follow the diagnostic from Perry to install/build that matching runtime; the CLI does not update Perry automatically.
- **A target is rejected:** inspect `perry compile --help` on that machine. Only targets advertised by that installed compiler are accepted.
- **Dependency installation failed:** the generated source files are retained. From the project directory, retry with the selected manager's `install` command.

## Releases

The CLI and engine have separate versions and release workflows. A CLI tag such as `v0.1.1` produces standalone release assets for Linux x86-64, Windows x86-64, macOS x86-64, and macOS ARM64. Alternatively, install from GitHub with Cargo as shown above.

## License

MIT. See [LICENSE](LICENSE).
