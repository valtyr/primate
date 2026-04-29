# Types

primate has a small, fixed set of built-in types. Users compose them
into structures with type constructors (`array`, `tuple`, etc.) and
name them with `type` aliases.

## Primitives

### Numbers

| Type        | Use                                                            |
|-------------|----------------------------------------------------------------|
| `i32` `i64` | Signed integers.                                               |
| `u32` `u64` | Unsigned integers.                                             |
| `i8` `i16`  | **Only as enum backing types in v1.** Widened to `i32` in IR. |
| `u8` `u16`  | **Only as enum backing types in v1.** Widened to `u32` in IR. |
| `f32` `f64` | Floats.                                                        |

The "only as enum backing" restriction reflects how rare 8/16-bit
constants are in cross-language code; widening to `i32`/`u32` keeps
generators simple. (When fixed-size arrays of `u8` are useful — e.g.
RGB triples — they're *value types*, not constants in the bit-twiddling
sense; see [fixed-size arrays](#fixed-size-arrays).)

#### A note on type fidelity

primate's numeric types are richer than what most targets natively
distinguish. Each generator preserves what the target supports and
widens the rest — `i32` and `u32` survive faithfully into Rust, but
land as `number` in TypeScript and `int` in Python. The full mapping:

| primate     | Rust                       | TypeScript                                | Python      |
| ----------- | -------------------------- | ----------------------------------------- | ----------- |
| `i8`–`i64`  | `i8` / `i16` / `i32` / `i64` | `number`                                | `int`       |
| `u8`–`u64`  | `u8` / `u16` / `u32` / `u64` | `number` (or `bigint` for `u64` opt-in) | `int`       |
| `f32` / `f64` | `f32` / `f64`            | `number`                                  | `float`     |
| `duration`  | `std::time::Duration`      | `number` (ms) or `Temporal.Duration`      | `timedelta` |
| `string`    | `&'static str`             | `string`                                  | `str`       |
| `regex`     | `&'static str`             | `string`                                  | `str`       |
| `url`       | `&'static str`             | `string`                                  | `str`       |

This widening only affects the *type annotation* in generated code;
the values themselves are bounds-checked against the declared primate
type at parse time, not at generation time. `u32 X = 5GiB` is an
`out-of-range` error before any generator sees it, even though the
TypeScript output would have been `number`.

### Boolean

```primate
bool ENABLED = true
bool DEBUG   = false
```

### `string`

UTF-8. Regular and raw forms:

```primate
string GREETING  = "Hello, world!"
string LITERAL   = "Has \"quotes\" and \\ backslashes."
string RAW       = r"no\\escapes\here"
string RAW_QUOTE = r#"with "quotes" inside"#
```

### `duration`

A length of time. primate normalizes durations to nanoseconds
internally.

```primate
duration TIMEOUT      = 30s
duration RETRY_WAIT   = 500ms
duration TICK         = 16ms
duration RUN_FOR      = 2h
duration LEASE        = 1d
duration PRECISION    = 1ns
```

Suffixes: `ns`, `us` (or `µs`), `ms`, `s`, `min`, `h`, `d`. Generators
emit per target — `std::time::Duration` in Rust, milliseconds-as-`number`
or `Temporal.Duration` in TypeScript (configurable), `timedelta` in
Python.

### Byte sizes are integer literals

Byte sizes aren't a separate type — they're sugar on integer literals.
A literal like `100MiB` is just `104_857_600` of whatever integer type
you declared:

```primate
u64 MAX_UPLOAD  = 100MiB
u32 BLOCK_SIZE  = 4KiB
u32 PACKET_SIZE = 1500B
```

Suffixes: `B`, `KB`/`MB`/`GB`/`TB` (decimal, ×1000), and
`KiB`/`MiB`/`GiB`/`TiB` (binary, ×1024). Allowed on `i32`, `i64`,
`u32`, and `u64` literals.

primate bounds-checks the suffix-multiplied result against the
declared type. `u32 X = 5GiB` is an `out-of-range` error because
5 GiB exceeds `u32::MAX`.

### `regex`

A regex pattern stored as a string. Validated at parse time.

```primate
regex FILENAME = "(?i)^[a-z][a-z0-9_]*\\.txt$"
```

Regex values are written as ordinary strings (not `/.../` literals).
This keeps `/` free for a future division operator.

### `url`

A URL string, validated at parse time.

```primate
url HOMEPAGE = "https://example.com"
```

## Type constructors

### `array<T>` — variable-length array

```primate
array<u32>    QUEUE_DEPTHS = [4, 8, 16, 32]
array<string> ALLOWED_HOSTS = ["api.example.com", "cdn.example.com"]
```

Sugar: `T[]` is equivalent to `array<T>`. The formatter prefers the
sugared form.

### `array<T, N>` — fixed-size array

```primate
type Pixel  = array<u32, 3>      // RGB triple
type Matrix = array<Pixel, 3>    // 3×3 grid
```

Length-mismatch is a hard error: `array<u32, 3> X = [1, 2]` produces a
`length-mismatch` diagnostic.

In Rust this generates `[T; N]`; in TypeScript and Python a
homogeneous tuple of N elements. See the
[matrices cookbook](../cookbook/matrices.md) for a worked example.

### `optional<T>`

```primate
optional<duration> RETRY_AFTER = 30s
optional<duration> NEVER       = none
```

Sugar: `T?`. Values are either a regular `T` literal or the keyword
`none`.

### `map<K, V>`

```primate
map<string, u32> SERVICE_PORTS = {
    "http":  80,
    "https": 443,
    "ssh":   22,
}
```

Map keys can be strings, identifiers, or integers; the value type is
arbitrary. Trailing comma triggers multi-line formatting (see
[Values](./values.md)).

### `tuple<A, B, …>`

```primate
type RetrySchedule = tuple<u32, duration, duration>

RetrySchedule DEFAULT = [3, 100ms, 30s]
```

Heterogeneous, fixed-arity. Tuple values use square brackets — see
[Values](./values.md) for the rationale.

## User-defined types

`enum` and `type` declarations introduce types you can use anywhere a
primitive type can go. `type` is *structural*: `type Port = u32` and
`u32` are interchangeable.

Enums and aliases live in their declaring file's namespace.
Cross-namespace references are by qualified path or via [`use`](./use.md):

```primate
core::types::LogLevel DEFAULT_LEVEL = Info
```

## Multi-line type expressions

Inside `<>`, newlines are insignificant. Long type expressions can wrap:

```primate
type ServiceConfig = map<
    string,
    tuple<duration, u64, optional<url>, regex>,
>
```

Trailing commas are accepted on the type side but don't trigger
multi-line formatting — type expressions tend to be short enough that
the column budget alone suffices.
