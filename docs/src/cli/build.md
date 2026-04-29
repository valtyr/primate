# `primate build`

Reads `primate.toml`, parses every `.prim` file under `input`, and
writes generated files per configured target.

```bash
primate build
```

Run from the directory containing `primate.toml` (or pass `--config
path/to/primate.toml`).

## Config file

```toml
# primate.toml
input = "constants"

[[output]]
generator = "typescript"
path      = "web/src/generated/constants/"   # directory

[[output]]
generator = "rust"
path      = "src/generated/constants.rs"     # file

[[output]]
generator = "python"
path      = "scripts/generated/constants/"   # directory
```

### Top-level keys

- `input` (required) — path to the directory of `.prim` files,
  relative to `primate.toml`. primate walks this dir recursively.
- `sourcemap` (optional, default `primate.sourcemap.json`) — where
  the IDE sourcemap is written. The sourcemap lets the LSP jump
  between source `.prim` lines and generated lines.

### `[[output]]` entries

Each entry enables one target. `generator` selects a built-in
(`rust`, `typescript`, `python`) or a plugin (see
[Plugins](../plugins/protocol.md)).

Common keys:

- `generator` (required) — generator name.
- `path` (required) — where primate writes output, relative to
  `primate.toml`. **TypeScript and Python expect a directory; Rust
  expects a file.**
- `options.<key>` — generator-specific options (see below).

## Built-in generators

### `typescript`

`path` is a directory. primate emits one `.ts` file per source-file
namespace plus an `index.ts` that re-exports each namespace. Cross-
namespace type references become real ES `import` statements at the
top of each file.

```toml
[[output]]
generator = "typescript"
path      = "web/src/generated/constants/"
options.naming    = "camelCase"      # or "SCREAMING_SNAKE_CASE"
options.duration  = "number"         # or "temporal" — emits Temporal.Duration values
options.u64       = "number"         # or "bigint"
options.enumStyle = "literal"        # or "const", "enum"
```

Defaults are conservative: `camelCase` constants, `number` durations
in milliseconds, `number` for `u64`, string-literal enums.

TypeScript doesn't distinguish between `i32`, `u32`, `i64`, `f64`,
etc. — they all land as `number`. Bounds checking happens at primate's
parse time against the declared type. See
[type fidelity](../language/types.md#a-note-on-type-fidelity).

### `rust`

`path` is a file. primate emits one `.rs` file with a `pub mod <ns>`
block per namespace. Cross-namespace references become
`super::<other>::X`.

```toml
[[output]]
generator = "rust"
path      = "src/generated/constants.rs"
options.visibility = "pub"           # or "pub(crate)", "pub(super)", ""
```

Rust is the highest-fidelity target — `i32`/`u32`/`i64`/`u64`/`f32`/
`f64` all survive as native types. See
[type fidelity](../language/types.md#a-note-on-type-fidelity).

### `python`

`path` is a directory. primate emits one `.py` file per source-file
namespace plus an `__init__.py` that re-exports each namespace as a
submodule. Cross-namespace references become relative imports
(`from .other import X`).

```toml
[[output]]
generator = "python"
path      = "scripts/generated/constants/"
options.typing = "runtime"           # or "stub" (emits a .pyi-style file)
```

Durations become `timedelta`. Integer-backed enums become `IntEnum`
subclasses; string-tagged enums become `(str, Enum)` subclasses.

Python doesn't distinguish between integer widths — `i32`, `u32`,
`i64`, and `u64` all land as `int`. Bounds checking happens at
primate's parse time against the declared type. See
[type fidelity](../language/types.md#a-note-on-type-fidelity).

## Why a directory for TS and Python, but a file for Rust?

Modules in TypeScript and Python *are* files: cross-module references
require an `import` from another file. To preserve module structure,
primate generates one file per namespace.

Rust expresses modules in-file with `pub mod <name> { ... }`, so a
single `.rs` file already preserves namespace boundaries. Generating
a directory would be needless ceremony.

## Behavior

`primate build`:

1. Walks `input` recursively, picking up every `*.prim` file.
2. Parses each file; reports diagnostics with file/line/column.
3. Lowers to IR (resolves cross-file types, `use` imports, alias
   chains).
4. Runs each enabled generator. Generators receive a JSON request on
   stdin and emit JSON on stdout (built-ins and plugins use the same
   protocol; see [Plugins](../plugins/protocol.md)).
5. Writes the generated files plus a sourcemap.

If any diagnostic is an error, primate exits non-zero and writes
nothing.

## Exit codes

- `0` — all generators succeeded.
- `1` — parse, lower, or generation error. Diagnostics on stderr.
- `2` — config or filesystem error (missing `primate.toml`,
  unwritable output path, etc.).

## CI usage

Most projects want to fail the build if generated files are stale:

```bash
primate fmt --check                  # all .prim files are formatted
primate build                        # regenerate
git diff --exit-code path/to/output  # generated files match what's checked in
```

Or use a build target that ensures `primate build` runs before tests.

## See also

- [`primate fmt`](./fmt.md) — formatter.
- [`primate lsp`](./lsp.md) — language server (used by editors).
- [Plugins](../plugins/protocol.md) — bring your own generator.
