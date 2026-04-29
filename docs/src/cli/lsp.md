# `primate lsp`

Starts the primate language server, speaking LSP over stdio. Editor
extensions invoke this — you don't usually run it directly.

```bash
primate lsp                    # search for primate.toml upward from cwd
primate lsp --config path      # use this config explicitly
```

## What the server does

| Capability                | Behavior                                                                          |
|---------------------------|-----------------------------------------------------------------------------------|
| Diagnostics               | Parse + lower the workspace; surface errors per file as you type.                 |
| Hover                     | Hovering a type name shows its kind, namespace, doc comment, and (for enums) variants. |
| Go-to-definition          | Click a type or qualified path → jumps to its declaration, including across files. |
| Find references           | Finds every place a type is used in the workspace, following `use` imports.       |
| Completion                | Type names and keywords in type position; literals in value position.             |
| Formatting                | `textDocument/formatting` runs the same logic as `primate fmt`.                   |
| Sourcemap navigation      | Custom requests for jumping between source `.prim` lines and generated lines.     |

## How it parses your workspace

The server walks each workspace folder for `.prim` files (just like
`primate build`). It maintains a content-hash cache per file so that
unchanged files aren't re-lexed/re-parsed on every keystroke. The
"lower" pass — cross-file type resolution, `use` resolution — runs on
every request, but it's the cheap part of the pipeline.

Result: typing in one file only re-parses *that file*, plus the
constant-time lower over the cached ASTs of every other file.

## Per-file vs workspace diagnostics

`primate lsp` parses the **whole workspace** to produce a file's
diagnostics. That's required for cross-file resolution: when you
write `core::types::LogLevel` in `app.prim`, the server only knows it's
valid by parsing `core/types.prim` too.

Diagnostics for the current file are filtered to those whose source
location lives in that file. If you've broken a sibling file, you'll
see its diagnostics on the next time you open it (not eagerly on every
buffer's change).

## Logging

The server logs to stderr with a `[LSP]` prefix:

```text
[LSP] Starting primate LSP server...
[LSP] Workspace folders: ["/Users/me/proj"]
[LSP] DidOpenTextDocument: file:///Users/me/proj/constants/limits.prim
[LSP] Completion request: ...
```

Editors capture this; check your editor's LSP log to inspect activity.

In Zed, that's `Zed.log`:

```bash
tail -f ~/Library/Logs/Zed/Zed.log
```

## See also

- Editors: [Zed](../editors/zed.md), [VS Code](../editors/vscode.md),
  [Vim](../editors/vim.md).
- [`primate fmt`](./fmt.md) — same formatting logic.
- [`primate build`](./build.md) — same parser/lower pipeline.
