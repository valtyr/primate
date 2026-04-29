# RFC 0004 — Byte-size sugar, attribute pruning, bounds checking

Status: Accepted
Date: 2026-04-29
Extends: RFC 0002, RFC 0003

## Summary

Three changes that sharpen primate's value model:

1. Drop the `bytes` type. Allow byte-size unit suffixes (`B`, `KiB`,
   `MiB`, …) on integer literals of `u32`, `u64`, `i32`, `i64`.
2. Drop `@only` and `@deprecated` from the built-in attribute set.
   Keep `@inline`; keep the attribute syntax open for plugins.
3. Bounds-check integer values against the declared primitive type.
   Out-of-range literals (including suffix-multiplied values) are an
   error, not a wraparound.

These are breaking changes — primate has no external users yet, so we
take the cleanups now.

## 1. Drop the `bytes` type; suffixes on integer literals

### Today

```primate
bytes MAX_UPLOAD = 100MiB
```

`bytes` is a primitive type. Generators emit `u64`/`number`/`int`.

### Problem

Two things:

- "Bytes" in Python (`bytes`) and Rust (`Vec<u8>`/`&[u8]`) means a
  blob of octets, not a byte-count. Reusing the name is a footgun.
- The type carries no runtime semantics — every target represents it
  as just an integer. Compare `duration`, which maps to
  `std::time::Duration` / `timedelta` / a target-configurable form
  with non-trivial conversion. `bytes` is purely sugar over `u64`.

### Proposed

Drop `bytes` as a type. Allow byte-size unit suffixes on integer
literals when the declared type is `i32`, `i64`, `u32`, or `u64`:

```primate
u64 MAX_UPLOAD  = 100MiB
u32 PACKET_SIZE = 1500B
u64 BLOCK_SIZE  = 4KiB
```

`100MiB` parses to `104_857_600`; the type is just `u64`. Same
generated code as before, modulo the type name change.

### Recognized suffixes

| Suffix | Meaning             |
|--------|---------------------|
| `B`    | byte                |
| `KB`   | 10³ bytes (decimal) |
| `MB`   | 10⁶                 |
| `GB`   | 10⁹                 |
| `TB`   | 10¹²                |
| `KiB`  | 2¹⁰ bytes (binary)  |
| `MiB`  | 2²⁰                 |
| `GiB`  | 2³⁰                 |
| `TiB`  | 2⁴⁰                 |

Suffixes are a parsing affordance: the literal `100MiB` desugars to
its multiplied value at lex time. The IR sees only the integer.

### Rule of thumb for type vs suffix

- A type earns inclusion when target languages have a first-class
  native representation that's nontrivial to convert.
- A suffix is sugar on number literals; it doesn't justify a type.

`duration` passes the bar (Rust `Duration`, Python `timedelta`).
`bytes` doesn't (just `u64`/`number`/`int` everywhere). Future
candidates like `frequency`, `percent`, `currency` would also fail
the type bar; if they ship at all, it's as suffixes.

### Other suffix categories

In v1 of this RFC, only byte-size suffixes are added to integer
literals. Duration suffixes (`ms`, `s`, `min`, …) remain restricted
to `duration`-typed contexts — using them on an integer literal is
a parse error. (We could relax that later if there's a reason, but
allowing `u64 X = 30s` is confusing — what's the unit?)

### Migration

Existing `bytes` declarations are a parse error; the diagnostic
suggests `u64`. Suffix syntax on the value side is unchanged.

## 2. Attribute pruning

### Today

Three built-in attributes: `@inline`, `@only(target, …)`,
`@deprecated(message?)`.

### Problem

`@only` and `@deprecated` were added to demonstrate that the
attribute system existed. In practice:

- `@only` covers a real edge case (single-platform constants), but
  the case is rare. Splitting into a per-target file is usually
  cleaner.
- `@deprecated` lets you mark constants going away, but for a
  *constants* language the right move is almost always to delete
  the constant. Marking it adds noise.

### Proposed

Drop `@only` and `@deprecated` from the built-ins. The attribute
syntax stays — it's still parsed, plugins can still read arbitrary
attributes, and any unknown attribute is a *warning* at parse time.

What remains as a built-in:

- `@inline` (on type aliases). Suppresses the alias from generated
  output and inlines the underlying type at use sites. This earns
  its keep — there's no other way to express "name this for
  clarity but don't surface it as a target type."

If `@deprecated` becomes a real need later, it's an additive
change.

### Migration

Existing `@only(...)` and `@deprecated(...)` annotations become
warnings (`unknown-attribute`). They have no effect on generated
output. Users can either delete them or move the corresponding
declaration to a per-target file.

## 3. Bounds checking on integer primitives

### Today

primate accepts any integer literal that fits in `i64` for any
integer type. `i32 X = 3_000_000_000` silently overflows the i32
range.

### Proposed

Validate every integer literal against the range of its declared
type. The same applies to suffix-multiplied values: `u32 X = 5GiB`
overflows `u32::MAX` and is an error.

| Type  | Range                              |
|-------|------------------------------------|
| `i32` | `−2_147_483_648 .. 2_147_483_647`  |
| `i64` | `i64::MIN .. i64::MAX`             |
| `u32` | `0 .. 4_294_967_295`               |
| `u64` | `0 .. u64::MAX`                    |

Same for enum backing types (`u8`/`u16` and signed counterparts):
each variant's value is bounds-checked against the backing type.

### Diagnostic

```
out-of-range: value 3_000_000_000 does not fit in i32 (range: -2_147_483_648..2_147_483_647)
```

Triggered for:

- Literals that exceed the declared type's range.
- Suffix-multiplied values that overflow.
- Negative literals on unsigned types (subsumes the existing
  "unsigned integer cannot be negative" error — the new code
  unifies them).
- Enum variant values outside the backing type's range.

### IR representation

`Value::Integer` switches from `i64` to `i128`. That covers every
declared primitive type — including `u64` near its max — without
sign-cast cleverness. Single match arm in generators; type
information is already carried alongside in `Type`, so we lose no
precision by widening the storage.

JSON serialization for the plugin protocol:

- Values that fit in `i64` → serialize as a JSON number (the common
  path; covers everything below ~9.2 × 10¹⁸).
- Values above `i64::MAX` but representable in `u64` →
  serialize as a JSON string. Plugins handle both shapes.
- Values above `u64::MAX` (only meaningful inside the `i128` range
  but for `u128`-sized types we don't have yet) → out of scope; if
  we ever add a wider integer type, the same string-when-large
  rule extends.

## Decisions deferred

(Consolidated to a single roadmap page in the docs; not repeated in
prose throughout the language reference.)

- `use a::b::C as D` rename imports.
- Nominal types (vs structural `type`). Might be implemented as an optional attribute @nominal or similar.
- Expressions and arithmetic.
- Other unit-suffix categories (`%`, `Hz`, `m`, currency).

## Decisions rejected

- **Keep `bytes` as a type.** Considered. The runtime-vs-sugar bar
  excludes it; the name is a footgun. Drop wins.
- **Add a `byte_size` (or similar) renamed type.** Considered. Adds
  a type that does no work the integer doesn't already do. Suffix
  on integer literals covers the actual ergonomic.
- **Allow duration suffixes on integer literals** (`u64 X = 30s`).
  Rejected — what's the unit? Without a declared `duration` type,
  the user has to remember whether `30s` desugars to seconds or
  nanoseconds. Easier to keep the type as the disambiguator.
- Glob imports (`use a::*`) will not be supported.
- Regex literal syntax `/.../flags`. The current string-with-`(?i)`
  form is the same approach Rust and Python use; we stick with it
  for now. Revisit if the inline-flag ergonomics become a recurring
  papercut.

## Implementation order

Independently shippable in this order:

1. **`i128` IR + bounds checking.** Switch `Value::Integer` to
   `i128`; add `out-of-range` diagnostic; check every integer
   literal in `normalize_value` and on enum variants. JSON
   serialization handles values above `i64::MAX`.
2. **Drop `@only`/`@deprecated`.** Remove the special-cases in
   lower; existing usages become `unknown-attribute` warnings.
3. **Drop `bytes` type, suffixes on integer literals.** Remove
   `Type::Bytes`; teach the integer normalization path to accept
   byte-size suffixes; update generators.

Each step is independently testable and shippable.
