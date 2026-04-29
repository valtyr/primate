# Zed

primate ships a Zed extension that wires up:

- a tree-sitter grammar for syntax highlighting,
- a language server pointing at the `primate lsp` binary.

The extension lives at `editors/zed/` in the project repo. Until it's
published to the Zed extension registry, you install it as a *dev
extension*.

## Install (dev extension)

### 1. Install the `primate` binary

Zed shells out to `primate lsp`. Install from a checkout:

```bash
cd <repo root>
cargo install --path . --locked
```

That puts `primate` in `~/.cargo/bin`. Make sure `~/.cargo/bin` is on
`$PATH`.

If you'd rather not install, point Zed at a custom binary path in
`settings.json`:

```jsonc
{
  "lsp": {
    "primate": {
      "binary": {
        "path": "/absolute/path/to/target/debug/primate"
      }
    }
  }
}
```

### 2. Install the dev extension

In Zed:

1. Open the command palette: `cmd-shift-p`.
2. Run **`zed: install dev extension`**.
3. Pick `editors/zed/` from this repo.

Zed compiles the extension (Rust → wasm), clones the tree-sitter
grammar from the project's monorepo, builds it to wasm, and reloads.
Open any `.prim` file to verify highlighting and LSP diagnostics.

### 3. Update the grammar `repository` URL

`editors/zed/extension.toml` has a `file://` URL for the grammar
source. After cloning the project repo, update that URL to the
absolute path of *your* checkout:

```toml
[grammars.primate]
repository = "file:///absolute/path/to/this/repo"
rev        = "main"
path       = "editors/zed/tree-sitter-primate"
```

This is the cleanest local-dev path: Zed clones the **project repo**
and finds the grammar source at the `path` sub-directory. No nested
`.git`, no commit-hash bumping. `rev = "main"` re-fetches the tip on
every rebuild.

## Iteration

| Change                                     | What to do                                       |
|--------------------------------------------|--------------------------------------------------|
| `editors/zed/src/lib.rs` (extension shim)  | `zed: rebuild dev extension`                     |
| `editors/zed/tree-sitter-primate/grammar.js` | `pnpx tree-sitter-cli generate`, commit, rebuild |
| `editors/zed/languages/primate/highlights.scm` | Just save — Zed reloads queries on edit       |
| `primate` binary (the LSP)                 | `cargo install --path . --locked` to refresh    |

To restart only the language server inside Zed: `editor: restart language server`.

## Verifying it works

Open one of `examples/constants/*.prim`. You should see:

- Keywords (`enum`, `type`, `namespace`, `use`) colored as keywords.
- Type names colored consistently in declarations and `use` statements.
- Unit suffixes on numeric literals (`30s`, `100MiB`) styled
  separately.
- Red squiggles on broken syntax.
- `cmd-.` formats the buffer (LSP `textDocument/formatting` runs the
  same logic as `primate fmt`).
- Hover on a type name shows its kind, namespace, and doc comment.
- Cmd-click on a type name jumps to its declaration.

If diagnostics don't show up, check `~/Library/Logs/Zed/Zed.log` —
the extension prints `[LSP] ...` messages on stderr.

## Publishing later

To turn this into a non-dev extension other people can install from
Zed's UI:

1. Push the project repo to GitHub.
2. Update `extension.toml` to the GitHub URL and pin a real commit
   SHA so installed users get a reproducible grammar build:

   ```toml
   [grammars.primate]
   repository = "https://github.com/<user>/primate"
   rev        = "<full-sha>"
   path       = "editors/zed/tree-sitter-primate"
   ```

3. Submit a PR to `zed-industries/extensions`.

## Why a tree-sitter grammar lives in this repo

primate ships its parser in Rust, but Zed (and other editors) need a
tree-sitter grammar for syntax highlighting and structural queries.
Maintaining a parallel tree-sitter grammar inside the project is the
pragmatic approach: it's tuned for highlighting (not for being a
spec-compliant parser), and it stays in sync with the Rust parser as
the language evolves.

If you spot a highlighting gap, it's almost always a fix in
`editors/zed/tree-sitter-primate/grammar.js` (the grammar) or
`editors/zed/languages/primate/highlights.scm` (the highlight rules).
