<img src="https://raw.githubusercontent.com/valtyr/primate/main/assets/logo.png" alt="primate" width="120" align="left" />

# primate — VS Code extension

VS Code language support for [`primate`](https://github.com/valtyr/primate),
a small DSL and code generator for cross-language constants. Write your
shared values once; generate typed Rust, TypeScript, and Python.

## Features

- Syntax highlighting for `.prim` files.
- LSP integration (talks to the `primate lsp` binary that ships with the
  CLI): diagnostics, hover docs, go-to-definition, find-references,
  format-on-save, contextual completion (enum variants, unit suffixes).
- Cross-target navigation: jump from a constant in a `.prim` file to its
  generated callsites in TypeScript / Rust / Python, and back.
- Schema validation for `primate.toml`.

## Setup

1. Install the [primate CLI](https://github.com/valtyr/primate):

   ```sh
   cargo install primate --locked
   ```

   The extension shells out to `primate lsp`. The CLI must be on `PATH`,
   or you can point the extension at a custom binary via the
   `primate.server.path` setting.

2. Install this extension from the VS Code Marketplace.

3. Open any `.prim` file. You should see syntax highlighting, and
   diagnostics appear inline as you type.

## Settings

| Setting              | Default     | Description                                                        |
| -------------------- | ----------- | ------------------------------------------------------------------ |
| `primate.server.path` | `"primate"` | Path to the `primate` executable. Defaults to looking on `PATH`.   |

## Commands

- **primate: Restart LSP Server** — kill and re-launch the language server,
  useful when you've upgraded the CLI binary.

## See also

- [Project README](https://github.com/valtyr/primate#readme)
- [Documentation](https://valtyr.github.io/primate/)
