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
