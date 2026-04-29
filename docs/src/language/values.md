# Values

primate value literals are deliberately compact and unambiguous. There
are no expressions; every constant is a literal of its declared type.

## Numeric literals

Integers can be decimal, hex, binary, or octal, with `_` as a digit
separator:

```primate
i32 SMALL    = 8
i32 BIG      = 1_000_000
i32 NEGATIVE = -5
i32 HEX      = 0xFF
i32 BINARY   = 0b1010
i32 OCTAL    = 0o755
```

Floats:

```primate
f64 PI       = 3.141_592
f64 SCIENT   = 1.5e10
f64 NEGATIVE = -0.5
```

Hex, binary, and octal literals **do not** accept unit suffixes (the
`30s` form). Unit suffixes only apply to decimal literals.

## Booleans

```primate
bool ON  = true
bool OFF = false
```

## Strings

Regular strings allow standard escapes (`\n`, `\r`, `\t`, `\0`, `\\`, `\"`):

```primate
string GREETING = "Hello, world!"
string PATH     = "C:\\Users\\val"
```

Raw strings have no escapes and can include unescaped quotes by adding
`#`s:

```primate
string PATTERN = r"raw\nstring"
string SQL     = r#"SELECT * FROM users WHERE name = "alice""#
```

## `duration` literals

```primate
duration FAST   = 50ms
duration SHORT  = 5s
duration MED    = 5min
duration LONG   = 2h
duration LEASE  = 1d
duration BACKUP = 1w
duration TINY   = 1us
duration TINIER = 100ns
```

Suffixes: `ns`, `us`, `ms`, `s`, `min`, `h`, `d`, `w`. (`m` is also
accepted as an alias for `min`.)

Negative durations are allowed via `-`:

```primate
duration BACKDATED = -1d
```

## Byte-size literals

Byte-size suffixes are sugar on integer literals (no separate type):

```primate
u32 SMALL  = 512B
u32 PACKET = 1500B
u64 UPLOAD = 100MiB
u64 DISK   = 1TB
```

Suffixes: `B`, `KB`/`MB`/`GB`/`TB` (decimal, ×1000), and
`KiB`/`MiB`/`GiB`/`TiB` (binary, ×1024). Allowed on `i32`, `i64`,
`u32`, `u64`. The suffix-multiplied result is bounds-checked against
the declared type — `u32 X = 5GiB` is `out-of-range`.

## Percentage literals

`%` divides by 100 and is allowed on any float literal (integer or
decimal) assigned to an `f32`/`f64`:

```primate
f64 ROLLOUT     = 5%       // 0.05
f64 OPACITY     = 12.5%    // 0.125
f64 SAMPLE_RATE = 100%     // 1.0
```

This is purely sugar — the generated value is a plain float in the
target language; the percentage notation only exists at the source
level for readability.

## `none`

For any `optional<T>`, `none` represents "absent":

```primate
optional<duration> RETRY_AFTER = none
optional<string>   FALLBACK    = "v1"
```

## Array and tuple literals — `[…]`

Both arrays (homogeneous) and tuples (heterogeneous) use square
brackets. The parser produces a single shape; the lower pass picks
array vs tuple based on the declared LHS type.

```primate
type V3 = array<u32, 3>
V3 RGB_RED = [255, 0, 0]

type RetrySchedule = tuple<u32, duration>
RetrySchedule DEFAULT = [3, 100ms]
```

Why `[…]` for both? Visually consistent: ordered collections all use
`[]`. Matches TypeScript's tuple syntax.

## Map literals — `{…}`

```primate
map<string, u32> PORTS = {
    "http":  80,
    "https": 443,
}
```

Map keys can be strings, bare identifiers (treated as strings), or
integers.

## Magic trailing comma

A *trailing comma* on the last element of a collection literal is a
signal to the formatter: keep this multi-line, even if it would fit on
one line.

```primate
type Mat3 = array<array<u32, 3>, 3>

// Compact: fits on one line, formatter keeps it inline.
Mat3 SMALL = [[1, 0], [0, 1]]

// Trailing comma → formatter keeps it expanded as written.
Mat3 IDENTITY = [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
]
```

This is the rule Prettier popularized: the user opts into multi-line
layout by typing one extra character. Round-trip is stable: the
formatter always emits a trailing comma when wrapping multi-line.

The rule applies to value-side `[…]` and `{…}` literals. Type-side
generic arguments (`tuple<A, B,>`) accept a trailing comma but don't
trigger multi-line formatting — types are usually short enough that
the column budget alone suffices.

## Enum-variant values

When the LHS type is an enum, the value is a variant:

```primate
enum LogLevel: u8 {
    Debug = 0,
    Info  = 1,
    Warn  = 2,
    Error = 3,
}

LogLevel DEFAULT_LEVEL = Info
LogLevel STRICT_LEVEL  = LogLevel::Warn
```

Both forms are accepted: bare (when the enum is in scope) or qualified.
Cross-namespace references use the full path: `core::types::LogLevel::Info`.

## Type-checking

primate checks every value against its declared type at lower time.
Common diagnostics you'll hit:

- `type-mismatch` — `u32 X = "foo"` (string for an integer)
- `length-mismatch` — `array<u32, 3> X = [1, 2]` (wrong arity)
- `invalid-enum-variant` — `LogLevel L = Bogus` (variant not in enum)

See [Diagnostics](../reference/diagnostics.md) for the full list.
