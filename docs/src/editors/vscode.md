# VS Code

primate ships a VS Code extension that provides syntax highlighting
and connects to the `primate lsp` server. The extension lives at
`editors/vscode/` in the project repo.

## Install (development)

The extension isn't on the marketplace yet. Install from source:

### 1. Install the `primate` binary

```bash
cd <repo root>
cargo install --path . --locked
```

VS Code's extension expects `primate` to be on `$PATH` so it can spawn
`primate lsp`.

### 2. Install the extension

```bash
cd editors/vscode
pnpm install
pnpm run package        # produces a .vsix
code --install-extension primate-*.vsix
```

Or run the extension in development mode:

1. Open `editors/vscode/` in VS Code.
2. Press `F5` — that launches an "Extension Development Host" window
   with the extension loaded.

## What you get

- Syntax highlighting via a TextMate grammar
  (`editors/vscode/primate.tmLanguage.json`).
- LSP diagnostics, hover, go-to-definition, find references,
  completion, and formatting via `primate lsp`.

## Configuration

The extension's contributions in `package.json`:

- `extensions: [".prim"]` — files with this extension auto-detect as
  primate.
- `aliases: ["primate"]` — language ID + display name.
- A "primate" command category for any commands the extension
  exposes.

## Troubleshooting

If the language server doesn't start, check the **Output** panel →
**primate** for errors. The extension launches `primate lsp` and pipes
LSP messages over stdio; if `primate` isn't on `$PATH`, it'll fail at
startup.

## Status

The VS Code extension is less polished than the Zed integration today.
Specifically:

- The TextMate grammar is hand-written and less accurate than
  tree-sitter's structural rules.
- A few features (e.g. some completion contexts) are LSP-driven
  uniformly; if VS Code shows different results from Zed, that
  difference is on the editor side, not the LSP.

## Publishing later

To publish to the VS Code marketplace:

1. Update `editors/vscode/package.json` with a real publisher and
   repository URL.
2. `pnpm run package` to produce a `.vsix`.
3. `vsce publish` (requires a Visual Studio Marketplace account).
