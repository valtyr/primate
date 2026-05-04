# primate

primate is a DSL that generates typed constants for Rust, TypeScript, and
Python from one source. Source files have a `.prim` extension; project
config lives in `primate.toml`.

## When this applies

Anything touching `.prim` files, `primate.toml`, or the `primate` CLI.

## Setup

Project root needs `primate.toml`:

```toml
input = "constants"

[[output]]
generator = "typescript"
path      = "web/src/generated/constants/"

[[output]]
generator = "rust"
path      = "src/generated/constants.rs"
```

`path` is a directory for `typescript` and `python` (one file per
namespace, plus an `index.ts` / `__init__.py`). For `rust` it's a single
`.rs` file with `pub mod` blocks. Source files (`.prim`) live under
`input`.

## Syntax

Type-first, no inference, one declaration per line:

```primate
u32      MAX_RETRIES = 5
duration TIMEOUT     = 30s
u64      MAX_UPLOAD  = 100MiB
string   API_VERSION = "v3"
bool     STRICT_MODE = true
f64      ROLLOUT     = 5%
```

Naming (parser-enforced):
- constants: `SCREAMING_SNAKE_CASE`
- types/enums/variants: `PascalCase`
- namespaces: `lower_snake_case`

Suffixes (bounds-checked against the declared type):
- `duration` — `ns`, `us`, `ms`, `s`, `min`, `h`, `d`, `w`
- integers — `B`, `KB`, `MB`, `GB`, `TB`, `KiB`, `MiB`, `GiB`, `TiB`
- floats — `%` (e.g. `5%` → 0.05)

Enums (string-tagged by default; integer-backed when given a backing
type):

```primate
enum LogLevel: u8 { Debug = 0, Info = 1, Warn = 2, Error = 3 }
enum Status { Pending, Active, Done }
```

Type aliases are structural — `type Port = u32` makes `Port` and `u32`
interchangeable:

```primate
type Port = u32
Port HTTP_PORT = 8080
```

Namespace = file path under `input/`. **Don't write `namespace foo`** —
let the directory layout drive it. Cross-namespace references use a
qualified path or `use`:

```primate
use logging::LogLevel
LogLevel DEFAULT = Info
```

Containers: `array<T>` (or `T[]`), `array<T, N>` fixed-size, `optional<T>`
(or `T?`), `tuple<A, B>`, `map<K, V>`. Tuple/array values use `[…]`, not
`(…)`.

`///` doc comments attach to the next declaration and propagate to
generated output.

## Workflow

After **every** edit to a `.prim` file:

1. **Always run `primate fmt`** before considering the change done.
   - `primate fmt path/to/file.prim` to format one file.
   - `primate fmt` to format every `.prim` under `input`.
   - primate has one canonical layout; CI typically gates on
     `primate fmt --check`. Skipping this will fail review.
2. Run `primate` (or `primate generate`) to regenerate the target
   files.
3. Commit the regenerated output alongside the source changes.

Useful one-shots:
- `primate check` — validate without writing output.
- `primate generate --watch` — interactive TUI watch mode (sessions only,
  not CI).

## Don't

- Use expressions or arithmetic in values (`60 * 60` doesn't parse).
- Write `namespace foo` unless overriding the path-derived default.
- Use `(…)` for tuple/array values; use `[…]`.
- Re-type the same constant in another language — that's the whole point.
- Skip `primate fmt` after edits.

## Reference

Full docs: https://valtyr.github.io/primate/
