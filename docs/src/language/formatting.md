# Formatting

primate ships a normative formatter: there's one canonical form for
any source. `primate fmt` rewrites a file to that form; the LSP can
format on save.

## Run it

```bash
primate fmt path/to/file.prim       # format one file in place
primate fmt                         # format everything under `input`
primate fmt --check                 # exit non-zero if any file would change
```

In supported editors, `format document` (or format-on-save, if
configured) runs `primate fmt` on the buffer.

## Rules at a glance

- **4 spaces.** No tabs anywhere.
- **One declaration per line.** No semicolons.
- **Single space** around `=` and after `:`.
- **Sugared types preferred.** `T[]` over `array<T>`, `T?` over
  `optional<T>`.
- **Trailing comma in multi-line collections** (arrays, maps, tuple
  values, enum bodies).
- **No trailing comma in single-line collections.**
- **Magic trailing comma** in value literals keeps multi-line layout —
  see [Values](./values.md).

## Alignment within groups

Consecutive declarations with no blank line between them form a *group*.
Within a group, the formatter aligns the type, name, and `=` columns:

```primate
duration TIMEOUT     = 30s
u32      MAX_RETRIES = 5
u64      MAX_UPLOAD  = 100MiB
```

A `///` doc block is part of the declaration that follows it and does
not break the group. A blank line breaks the group. A standalone `//`
comment on its own line breaks the group.

Enum bodies follow the same rule — variants align, and `=` aligns when
any variant has an explicit value:

```primate
enum LogLevel: u8 {
    Debug = 0,
    Info  = 1,
    Warn  = 2,
    Error = 3,
}
```

## Long-line wrapping

When a logical line would exceed **column 100**, the formatter wraps at
the shallowest delimiter that lets the line fit:

```primate
// Before: 134 columns.
type ServiceConfig = map<string, tuple<duration, u64, optional<url>, regex, string>>

// After.
type ServiceConfig = map<
    string,
    tuple<duration, u64, optional<url>, regex, string>,
>
```

When a line is wrapped:

- One item per line.
- Trailing comma on the last item.
- Inner contents indented +4 from the line that opened the delimiter.

The wrapper recurses if the inner line is *also* over budget.

The 100-column budget is fixed.

## `use` block normalization

The block of `use` statements at the top of a file is normalized:

- Single-item brace groups collapse: `use a::b::{X}` → `use a::b::X`.
- Same-path `use` lines merge: `use a::b::X` + `use a::b::Y`
  → `use a::b::{X, Y}`.
- Top-level `use` lines sort by path.
- Items inside a brace group sort lexicographically.

A leading comment on a `use` line *pins* that line — sort/merge
happens within contiguous comment-free runs.

See [`use` statements](./use.md) for examples.

## What the formatter doesn't do

- It doesn't reorder declarations (only `use` blocks are sorted).
- It doesn't fix naming-convention violations — the parser flags those
  as `naming-convention` diagnostics; you fix them by hand.
- It doesn't rewrite literals (`100MiB` stays `100MiB`; not normalized
  to `1024 * 100`).
- It doesn't desugar `T?` → `optional<T>` (the sugar is preferred).

## Config

`primate fmt` has no command-line knobs in v1. Output is fully determined
by the formatter rules above. This is intentional: one canonical form,
no `.editorconfig`-style negotiation.
