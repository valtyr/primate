# primate — Zed extension

Zed language support for `.prim` files (the primate DSL).

The extension wires up:

- a tree-sitter grammar (`tree-sitter-primate/`) for syntax highlighting
- a language server pointing at the `primate lsp` binary

## Local install (dev extension)

The first time you set this up, you need to do three things:

### 1. Build the `primate` binary and put it on `PATH`

The Zed extension shells out to `primate lsp`. The simplest path is to install
it from a local checkout:

```bash
cd <repo root>
cargo install --path . --locked
```

That puts a release `primate` in `~/.cargo/bin`. If `~/.cargo/bin` is on your
`PATH`, you're done.

If you'd rather not install, you can point Zed at a custom binary path. Add
this to your Zed `settings.json`:

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

### 2. Tree-sitter grammar (already wired up)

Zed loads tree-sitter grammars from a git repository. We use Zed's
monorepo-friendly setup: `extension.toml` points at the **outer project
repo** via a `file://` URL, with `path` resolving the grammar source to
`editors/zed/tree-sitter-primate/`. The grammar source is a plain
directory tracked by the project repo — no nested `.git`, no separate
clone.

```toml
# editors/zed/extension.toml
[grammars.primate]
repository = "file:///absolute/path/to/this/repo"
rev        = "main"
path       = "editors/zed/tree-sitter-primate"
```

`rev = "main"` means Zed re-fetches whatever's at the tip of `main` on
each rebuild. **No commit-hash bumping**: when you change the grammar,
just commit your change to the project repo and rebuild the extension.

You'll need to update the `repository` URL once after cloning to match
your local checkout's path. Zed needs an absolute path.

#### Grammar iteration loop

```bash
# 1. Edit editors/zed/tree-sitter-primate/grammar.js
# 2. Regenerate the parser:
cd editors/zed/tree-sitter-primate
pnpx tree-sitter-cli generate

# 3. Sanity-check the grammar:
pnpx tree-sitter-cli parse ../../../examples/constants/limits.prim
# Expect a tree with no (ERROR ...) nodes.

# 4. Commit to the project repo (NOT the grammar dir — there's no inner .git):
cd ../../..
git add editors/zed/tree-sitter-primate
git commit -m "update grammar"

# 5. In Zed: command palette -> "zed: rebuild dev extension"
```

Zed shallowly fetches `main` from your local repo and rebuilds the
grammar wasm. No hash to bump.

### 3. Install as a dev extension

In Zed:

1. Open the command palette: `cmd-shift-p`
2. Run `zed: install dev extension`
3. Pick this directory (`editors/zed/`)

Zed will compile the extension (Rust → wasm), build the grammar, and reload.
Open any `.prim` file to verify highlighting and LSP diagnostics.

## Iterating

When you change something in the extension:

| Change                                      | What to do                                        |
| ------------------------------------------- | ------------------------------------------------- |
| `src/lib.rs` (extension wrapper)            | `zed: rebuild dev extension` from the palette     |
| `tree-sitter-primate/grammar.js`            | `pnpx tree-sitter-cli generate`, then rebuild     |
| `languages/primate/highlights.scm`          | Just save — Zed reloads queries on edit           |
| `primate` Rust source (the LSP server)      | `cargo install --path . --locked` to refresh bin  |

To restart just the language server inside Zed: `editor: restart language server`.

## Verifying it works

Open `examples/constants/limits.prim` from the repo root. You should see:

- `enum`, `type`, `namespace` highlighted as keywords
- `Pending`, `Active`, `Done` highlighted as enum variants
- `30s`, `100MiB` rendered with the unit suffix in a different colour
- A red squiggle if you introduce a typo (e.g. delete a `=`)
- `cmd-.` formats the buffer (LSP `textDocument/formatting` runs `primate fmt`)

If diagnostics don't appear, check `~/Library/Logs/Zed/Zed.log` (macOS) — the
extension prints `[LSP] ...` messages on stderr.

## Publishing later

To turn this into a non-dev extension other people can install:

1. Push the project repo to GitHub.
2. In `extension.toml`, switch the grammar repository to the GitHub URL
   and pin a commit SHA so installed users get a reproducible build:

   ```toml
   [grammars.primate]
   repository = "https://github.com/your-org/primate"
   rev        = "<full-sha>"
   path       = "editors/zed/tree-sitter-primate"
   ```

   (You can also split the grammar out into its own repo —
   `tree-sitter-primate` — and drop the `path` field. Either works.)

3. Submit a PR to `zed-industries/extensions`.
