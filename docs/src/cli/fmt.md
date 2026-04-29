# `primate fmt`

Rewrites `.prim` files to canonical form. There's exactly one canonical
form, defined by [Formatting](../language/formatting.md). `primate fmt`
has no formatter knobs.

## Usage

```bash
primate fmt path/to/file.prim       # format one file in place
primate fmt path/to/dir             # format every .prim file under dir
primate fmt                         # format everything under `input`
```

`primate fmt` reads `primate.toml` to find the `input` directory.

## Flags

- `--check` — don't write; exit non-zero if any file would change.
  Use this in CI.
- `--stdin` — read source from stdin and write the formatted output
  to stdout. Used by editor "format buffer" integrations.

## Examples

Format on save in CI-friendly form:

```bash
# fail the job if anything's unformatted
primate fmt --check
```

One-off pipe:

```bash
cat draft.prim | primate fmt --stdin > clean.prim
```

## Behavior

`primate fmt` parses each file. If parsing produces errors, the
formatter refuses to rewrite and surfaces the diagnostics — it won't
rewrite a file it can't parse cleanly.

The generated output is byte-for-byte deterministic given the input,
so the formatter is idempotent: running it twice changes nothing.

## See also

- [Formatting](../language/formatting.md) — the rules.
- [`primate build`](./build.md) — generates the output files.
- [`primate lsp`](./lsp.md) — the LSP exposes formatting via
  `textDocument/formatting`, which uses the same logic.
