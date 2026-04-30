# Zed

primate ships a Zed extension that wires up:

- a tree-sitter grammar for syntax highlighting,
- a language server pointing at the `primate lsp` binary.

Source lives at
[`editors/zed/`](https://github.com/valtyr/primate/tree/main/editors/zed)
in the project repo.

## Install

1. Install the `primate` binary so the extension can shell out to
   `primate lsp`:

   ```bash
   cargo install primate --locked
   ```

   The binary lands at `~/.cargo/bin/primate`. As long as that's on
   your `PATH`, you're set. Otherwise, point Zed at a custom binary
   path in `settings.json`:

   ```jsonc
   {
     "lsp": {
       "primate": {
         "binary": {
           "path": "/absolute/path/to/primate"
         }
       }
     }
   }
   ```

2. Install the extension from the Zed registry:

   - Command palette → **`zed: extensions`** → search "primate" → Install.

3. Open any `.prim` file. Syntax highlighting and inline diagnostics
   should appear immediately.

## What you get

- Tree-sitter syntax highlighting for `.prim` files (keywords, type
  names, enum variants, unit suffixes on numeric literals).
- LSP-driven diagnostics, hover, go-to-definition, find-references,
  format-on-save, and contextual completion.

## Verifying it works

Open one of `examples/constants/*.prim` from a checkout. You should
see:

- Keywords (`enum`, `type`, `namespace`, `use`) styled as keywords.
- Type names highlighted consistently in declarations and `use`
  statements.
- Unit suffixes (`30s`, `100MiB`) styled separately from the digits.
- Red squiggles on broken syntax.
- `cmd-.` formats the buffer (LSP `textDocument/formatting` runs the
  same logic as `primate fmt`).
- Hover on a type name shows its kind, namespace, and doc comment.
- Cmd-click on a type name jumps to its declaration.

If diagnostics don't show, check `~/Library/Logs/Zed/Zed.log` — the
extension prints `[LSP] ...` messages on stderr. The usual failure is
"`primate` not on PATH"; either `cargo install primate --locked` or
set `lsp.primate.binary.path`.

## Install as a dev extension

If you're hacking on the extension itself rather than just using it,
install the local source as a dev extension:

1. Clone the project repo.
2. In `editors/zed/extension.toml`, change the grammar `repository`
   from the GitHub URL to a `file://` URL pointing at your local
   checkout (the registry build clones over HTTPS, but `file://` works
   for local dev):

   ```toml
   [grammars.primate]
   repository = "file:///absolute/path/to/this/repo"
   rev        = "main"
   path       = "editors/zed/tree-sitter-primate"
   ```

3. In Zed: command palette → **`zed: install dev extension`** → pick
   `editors/zed/`.

Iterating:

| Change                                         | What to do                                       |
| ---------------------------------------------- | ------------------------------------------------ |
| `editors/zed/src/lib.rs` (extension shim)      | `zed: rebuild dev extension`                     |
| `editors/zed/tree-sitter-primate/grammar.js`   | `pnpx tree-sitter-cli generate`, commit, rebuild |
| `editors/zed/languages/primate/highlights.scm` | Just save — Zed reloads queries on edit          |
| `primate` binary (the LSP)                     | `cargo install --path . --locked` to refresh     |

To restart only the language server inside Zed: `editor: restart language server`.

## Why a tree-sitter grammar lives in this repo

primate ships its parser in Rust, but Zed (and other editors) need a
tree-sitter grammar for syntax highlighting and structural queries.
The parallel grammar in `editors/zed/tree-sitter-primate/` is tuned
for highlighting, not for being a spec-compliant parser. It stays in
sync with the Rust parser as the language evolves.

Highlighting gaps are almost always a fix in `grammar.js` (the
grammar) or `languages/primate/highlights.scm` (the highlight rules).
