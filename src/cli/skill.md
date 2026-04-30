# primate

DSL that generates typed constants for Rust, TypeScript, and Python from
one source. https://github.com/valtyr/primate

## Setup

```toml
# primate.toml at project root
input = "constants"

[[output]]
generator = "typescript"
path      = "web/src/generated/constants/"

[[output]]
generator = "rust"
path      = "src/generated/constants.rs"
```

`path` is a directory for `typescript`/`python`, a file for `rust`.

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

Namespace = file path under `input/`. Don't write `namespace foo` —
let the directory layout drive it. Cross-namespace via path or `use`:

```primate
use logging::LogLevel
LogLevel DEFAULT = Info
```

Containers: `array<T>` (or `T[]`), `array<T, N>` fixed-size,
`optional<T>` (or `T?`), `tuple<A, B>`, `map<K, V>`. Tuple/array
values use `[…]`, not `(…)`.

Doc comments (`///`) attach to the next declaration and propagate to
generated output.

## Commands

- `primate` — build (default).
- `primate fmt` / `--check` — canonical formatter, no flags.
- `primate check` — diagnostics only.
- `primate generate --watch` — TUI watch mode.

## Don't

- Use expressions or arithmetic in values (`60 * 60` doesn't parse).
- Write `namespace foo` unless overriding the path-derived default.
- Use `(…)` for tuple/array values.
- Re-type the same constant in another language — that's the whole
  point of primate; generate it.

## Reference

Full docs: https://valtyr.github.io/primate/
