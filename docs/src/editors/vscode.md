# VS Code

primate ships a VS Code extension that provides syntax highlighting
and connects to the `primate lsp` server. Source lives at
[`editors/vscode/`](https://github.com/valtyr/primate/tree/main/editors/vscode)
in the project repo.

## Install

1. Install the `primate` binary so the extension can shell out to
   `primate lsp`:

   ```bash
   cargo install primate --locked
   ```

   The binary lands at `~/.cargo/bin/primate`. As long as that's on
   your `PATH`, you're set. If you'd rather point the extension at a
   custom path, set `primate.server.path` in your VS Code settings.

2. Install the extension from the Marketplace:

   - <https://marketplace.visualstudio.com/items?itemName=valtyr.primate-vscode>
   - Or in VS Code: **Extensions** sidebar → search "primate" → Install.

3. Open any `.prim` file. Syntax highlighting and inline diagnostics
   should appear immediately.

## What you get

- Syntax highlighting via a TextMate grammar
  (`editors/vscode/primate.tmLanguage.json`).
- LSP-driven diagnostics, hover, go-to-definition, find-references,
  format-on-save, and contextual completion (enum variants, unit
  suffixes).
- **Cross-target navigation**: from a constant in a `.prim` file, find
  references jumps to its callsites in generated TypeScript / Rust /
  Python; from a generated symbol, go-to-definition resolves back to
  the originating `.prim` line via the sourcemap.
- JSON schema validation for `primate.toml`.

## Settings

| Setting               | Default     | Description                                                    |
| --------------------- | ----------- | -------------------------------------------------------------- |
| `primate.server.path` | `"primate"` | Path to the `primate` executable. Defaults to looking on PATH. |

## Commands

- **primate: Restart LSP Server** — kill and re-launch the language
  server. Useful when you've upgraded the CLI binary.

## Troubleshooting

If the extension activates but diagnostics never show, the language
server probably failed to start. Check **Output** panel → **primate
LSP** for the error message. Common causes:

- `primate` isn't on `PATH`. Run `which primate` from a terminal; if
  empty, either `cargo install primate --locked` or set
  `primate.server.path` to an absolute path.
- The CLI version doesn't match the extension. Run
  `primate --version` to confirm. Older CLIs may not implement
  some LSP requests the extension expects (e.g. cross-target
  navigation is added in v0.1+).

## Install from source (development)

If you're hacking on the extension itself rather than just using it:

```bash
cd editors/vscode
npm install
npm run compile
```

Then in VS Code, open `editors/vscode/` and press **F5** — that
launches an "Extension Development Host" window with the in-progress
extension loaded.

To package a `.vsix` locally without publishing:

```bash
npx --yes @vscode/vsce package
code --install-extension primate-vscode-*.vsix
```
