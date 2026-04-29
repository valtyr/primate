# Diagnostics

Every error and warning primate emits has a stable code. Codes show up
in the LSP, in `primate build` output, and in CI failure messages.
This page lists every code, what triggers it, and how to fix it.

## Parse layer

### `parse-error`

A token didn't fit the grammar. The error message describes what was
expected vs what was found.

```primate
u32 X = 8;            // ✗ semicolons aren't allowed
                      //   parse-error: unexpected character ';'
```

**Fix:** match the grammar. See the [grammar reference](./grammar.md).

### `parse-failure`

A higher-level structural failure where the parser bailed on a whole
declaration. Usually accompanied by a more specific `parse-error`.

## Naming

### `naming-convention`

A name doesn't match its declaration's case convention.

```primate
u32 maxRetries = 5    // ✗ constants are SCREAMING_SNAKE_CASE
                      //   naming-convention: constant `maxRetries` must be SCREAMING_SNAKE_CASE
```

**Fix:** rename to the convention.

| Item            | Convention             |
|-----------------|------------------------|
| Constants       | `SCREAMING_SNAKE_CASE` |
| Enums           | `PascalCase`           |
| Enum variants   | `PascalCase`           |
| Type aliases    | `PascalCase`           |
| Namespaces      | `lower_snake_case`     |

## Resolution

### `unknown-type`

A type name doesn't resolve to a primitive, an in-scope user type, or
an imported name.

```primate
SomeMissingType X = 0
// ✗ unknown-type: unknown type `SomeMissingType`
```

**Fix:** check the spelling, ensure the type is declared (or imported),
or qualify with the namespace.

### `duplicate-name`

Two declarations with the same name in the same namespace.

```primate
type Port = u32
type Port = u16    // ✗ duplicate-name: type alias `Port` is already declared
```

**Fix:** rename one, or move them into different namespaces.

### `duplicate-namespace`

A file has more than one `namespace` line, or a `namespace` line is
not at the top.

**Fix:** keep at most one `namespace` line, and place it as the first
non-comment item in the file.

## Type-checking

### `type-mismatch`

A value doesn't match its declared type.

```primate
u32 X = "hello"
// ✗ type-mismatch: expected integer, got string literal
```

**Fix:** correct the value, or change the declared type.

### `length-mismatch`

A fixed-size array literal has the wrong arity.

```primate
array<u32, 3> X = [1, 2]
// ✗ length-mismatch: expected 3 elements for array<_, 3>, got 2
```

**Fix:** add or remove elements, or change the declared length.

### `out-of-range`

A literal — or the result of multiplying it by a unit suffix — exceeds
the declared primitive type's range. Subsumes the older
"unsigned integer cannot be negative" error.

```primate
i32 X = 3_000_000_000
// ✗ out-of-range: value 3000000000 does not fit in i32 (range: -2147483648..=2147483647)

u32 Y = 5GiB
// ✗ out-of-range: value 5368709120 does not fit in u32 (range: 0..=4294967295)

u32 Z = -1
// ✗ out-of-range: value -1 does not fit in u32 (range: 0..=4294967295)
```

Same code applies to enum variant values that overflow the backing
type:

```primate
enum Big: u8 {
    A = 0,
    B = 300,   // ✗ out-of-range: value 300 does not fit in u8
}
```

**Fix:** widen the declared type, or use a smaller value.

### `invalid-enum-backing`

The `: <type>` on an enum is not an integer primitive.

```primate
enum Bad: string {
//      ✗ invalid-enum-backing: enum backing type must be an integer
    A,
    B,
}
```

**Fix:** drop the backing for a string-tagged enum, or set it to one
of `i8`/`i16`/`i32`/`i64`/`u8`/`u16`/`u32`/`u64`.

### `invalid-enum-variant`

A value typed as an enum doesn't match any variant.

```primate
enum Status { Pending, Active }

Status X = Done
// ✗ invalid-enum-variant: `Done` is not a variant of enum `Status`
```

**Fix:** use one of the listed variants, or add `Done` to the enum.

## `use` and imports

### `unresolved-import`

A `use` statement references a name that doesn't exist.

```primate
use net::limits::Bogus
// ✗ unresolved-import: `net::limits::Bogus` does not exist
```

**Fix:** check the path and the imported name.

### `import-collision`

A `use` brings in a name that collides with another import or with a
same-namespace declaration.

```primate
use net::limits::Port
use audio::limits::Port
// ✗ import-collision: `Port` is already imported from `net::limits`
```

**Fix:** import only one, or qualify both at the call site.

## Config

### `config-error`

`primate.toml` is malformed or references a missing path.

**Fix:** the message points at the offending key. Common causes: a
missing `input` key, an unwritable `output` path, or a generator
without an `output`.

## Internal

### `internal-error`

primate hit an unexpected state inside the parser, lower, or
generator. This is a bug — please report it with the offending source
file (or a minimal repro).
