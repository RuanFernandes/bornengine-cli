# @bornengine/cli

Install the BornEngine command line tools with npm:

```sh
npm install --global @bornengine/cli
bornengine perry install
bornengine create
```

The launcher downloads the matching, checksum-verified native executable on first use. The binary is cached per user and reused by later invocations. Supported release targets are Linux x86-64, Windows x86-64, macOS x86-64, and macOS ARM64. Node.js 18 or newer is required to launch the npm-installed command. `bornengine perry install` downloads the Perry compiler that game builds use, and game projects also need a package manager.

To copy the language model guide into the current directory, use the Rust CLI's `--add-ai-docs` option:

```sh
bornengine --add-ai-docs assistant-guide
```

The command writes `assistant-guide.md` and refuses to overwrite an existing file.

See the [BornEngine CLI repository](https://github.com/RuanFernandes/bornengine-cli) for all commands and build requirements.
