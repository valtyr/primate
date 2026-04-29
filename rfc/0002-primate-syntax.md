# RFC 0002 — primate Source Syntax

Status: Accepted
Date: 2026-04-28
Replaces: TOML-based input format from RFC 0001

## Summary

Replace the TOML-based `.c.toml` input format with a purpose-built DSL using
the `.prim` file extension. The DSL is type-first, declaration-only, and
designed to give us room to add expressions, imports, and richer types later
without breaking changes.

This RFC is the source of truth for syntax decisions. Implementation lives
in `src/lexer/`, `src/parser/`, `src/ast/`, and `src/formatter/`.

## File extension

`.prim`

No collisions with major languages or tooling. Some assemblers use `.prim`
as an in-source section directive, but not as a file extension.

## Top-level shape

A file is a sequence of items separated by newlines. There are five item
kinds:

1. `namespace` declaration (zero or one per file, must be first non-comment
   item if present)
2. `enum` declaration
3. `type` alias declaration
4. Constant declaration
5. Comments (line, doc, file-doc)

No imports in v1. No expressions in v1.

## Comments

```primate
// Line comment.
/// Doc comment — attaches to the next declaration.
//! File doc — only valid before any declaration.
```

There are no block comments. C-family `/* */` is rejected with a clear
diagnostic so the lexer remains regular and so we never have nested-comment
arguments.

`///` blocks attach to the *immediately following* declaration. A blank line
between a doc block and a declaration detaches it (the doc block becomes a
standalone comment, which is not a parse error but emits a warning).

## Identifiers and naming

- Constants: `SCREAMING_SNAKE_CASE`
- Enums and type aliases: `PascalCase`
- Enum variants: `PascalCase`
- Namespaces: `lower_snake_case` segments, separated by `::`

The parser enforces the case conventions and surfaces a `naming-convention`
diagnostic on violation.

## Namespace declaration

```primate
namespace core::time
```

- One per file, zero allowed (defaults to the directory-derived path).
- Single line, no braces — avoids C#/Java nested-namespace pyramids.
- Path uses `::` as separator. Filesystem still uses `/`, so
  `constants/core/time.prim` defaults to `namespace core::time`.

Files sharing a namespace share a flat scope: enums and type aliases declared
in one are visible in all, by bare name.

## Constant declarations

```primate
<type> <NAME> = <literal>
```

- `<type>` is mandatory (no inference). Required to prevent the type-creep
  regret pattern of Go `:=`, TS implicit `any`, and Python untyped scope.
- `<NAME>` is `SCREAMING_SNAKE_CASE`.
- One per line. Newline ends the declaration.
- No semicolons. Newlines terminate; that decision will hold under
  expressions because expressions only span lines inside `(`, `[`, `{` or
  trailing operators (Python's rule, not JS's ASI).

### Types

Built-in primitives:

| Type | Notes |
|------|-------|
| `i32` `i64` | Signed integers |
| `u32` `u64` | Unsigned integers |
| `i8` `i16` `u8` `u16` | Accepted **only** as enum backing types in v1; widened to `i32`/`u32` in the IR |
| `f32` `f64` | Floats |
| `bool` | `true`, `false` |
| `string` | UTF-8 |
| `duration` | Normalized to nanoseconds internally |
| `bytes` | Normalized to bytes internally |
| `regex` | Validated regex pattern stored as a string |
| `url` | Validated URL string |

Type constructors:

| Form | Meaning |
|------|---------|
| `T[]` | Array (sugar for `array<T>`) |
| `T?` | Optional (sugar for `optional<T>`) |
| `array<T>` | Array, explicit form |
| `optional<T>` | Optional, explicit form |
| `map<K, V>` | Map |
| `tuple<A, B, ...>` | Heterogeneous tuple |

Both sugared and explicit forms are accepted by the parser and the formatter
canonicalizes to the sugared form when applicable.

Qualified types reference enums or aliases in another namespace:

```primate
core::types::LogLevel DEFAULT_LEVEL = Info
```

### Literals

| Kind | Examples |
|------|----------|
| Integer | `8`, `-5`, `1_000_000`, `0xFF`, `0b1010`, `0o755` |
| Float | `3.14`, `-0.5`, `1.5e10`, `1_000.5` |
| Boolean | `true`, `false` |
| String | `"hello"`, raw `r"no\\escapes"`, `r#"with "quotes""#` |
| Duration | `30s`, `500ms`, `2h`, `1d`, `1us`, `1ns`, `5min` |
| Bytes | `100MiB`, `50KB`, `1GiB`, `512B`, `2TB` |
| Array | `[1, 2, 3]`, trailing comma OK |
| Map | `{"key": value, "other": value}`, trailing comma OK |
| Tuple | `(1, "a", true)`, trailing comma OK |
| Optional | `none`, or any value of the inner type |
| Enum variant (when LHS is an enum) | bare `Info`, or qualified `core::LogLevel::Info` |

Underscores are permitted as digit separators. Hex/binary/octal literals
**must not** carry unit suffixes; only decimal literals do.

Regex values are written as ordinary strings:

```primate
regex FILENAME = "(?i)^[a-z][a-z0-9_]*\\.txt$"
```

This is intentional — it keeps `/` free for a future division operator
without context-sensitive lexing (the JS/Perl/Ruby regret).

## Top-level enums

```primate
/// Operation status.
enum Status {
    Pending,
    Active,
    Done,
}
```

Optional backing type for integer-valued enums:

```primate
/// Severity, backed by u8.
enum LogLevel: u8 {
    Debug = 0,
    Info  = 1,
    Warn  = 2,
    Error = 3,
}
```

- Backing type must be an integer primitive when present.
- Variants without explicit values get auto-assigned (0, 1, 2, ...) when a
  backing type is present; without a backing type, variants are tagged
  string-named.
- Enums are visible across files in the same namespace; cross-namespace
  references use a qualified path.

## Type aliases

```primate
type Port          = u16
type ServiceConfig = map<string, Port>
type Color         = tuple<u8, u8, u8>
```

Aliases are emitted as standalone type definitions in generated code by
default — that's the whole reason they have names.

To suppress emission and expand at use sites, mark with `@inline`:

```primate
@inline
type Bytes32 = bytes
```

Aliases participate in cross-file resolution exactly like enums.
Alias-of-alias chains are resolved transitively at IR time, so generated
code never contains a chain.

## Attributes

```primate
@deprecated("use NEW_TIMEOUT instead")
duration OLD_TIMEOUT = 30s

@only(typescript, rust)
u32 PLATFORM_SPECIFIC = 4

@inline
type Bytes32 = bytes
```

Form: `@name` or `@name(arg, arg, ...)` where each `arg` is a literal or a
bare identifier. Attributes attach to the next declaration. Multiple
attributes per declaration stack on separate lines.

v1 honors:

- `@deprecated(message?)` — emits a deprecation warning in target output
  where supported (e.g., `@deprecated` JSDoc, Rust `#[deprecated]`).
- `@only(target, ...)` — restrict generation to listed generators.
- `@inline` — on type aliases only; suppress emission, expand at use site.

The `@name(args)` form is fully reserved at parse time. Unknown attribute
names emit a warning, not an error, so plugins can introduce custom
attributes without forking the parser.

## Resolution rules

1. **Same file:** all top-level enums and aliases visible.
2. **Same namespace, sibling files:** all top-level enums and aliases
   visible by bare name.
3. **Cross-namespace:** reference by fully qualified path
   (`core::types::LogLevel`). No `use` in v1; reserved as a future additive
   feature.

Duplicate declarations across files within the same namespace are an error,
with diagnostics pointing at both source locations.

## Formatting rules

The formatter is normative. There is one canonical form. Specifics:

- Indentation: 4 spaces. No tabs anywhere.
- One declaration per line. No semicolons.
- Single space around `=` and after `:`.
- Trailing comma in multi-line arrays, maps, tuples, enum bodies.
- No trailing comma in single-line collections.
- Type aliases: sugar form (`T[]`, `T?`) preferred over explicit
  (`array<T>`, `optional<T>`).

### Alignment within groups

Consecutive declarations with no blank line between them form a *group*.
Within a group:

- Type column padded to widest type in the group.
- Name column padded to widest name in the group.
- `=` column aligned.
- Values left-flushed.

A `///` doc block is part of the declaration that follows it and does not
break the group.

A standalone `//` comment on its own line breaks the group.
A blank line breaks the group.

Example:

```primate
/// Time the app waits before bailing.
duration TIMEOUT             = 30s
duration RETRY_INTERVAL      = 500ms
u32      MAX_CONNECTED_USERS = 8
bytes    MAX_UPLOAD_SIZE     = 100MiB
```

Enum bodies follow the same alignment within their braces — variants
align, and `=` aligns when any variant has an explicit value.

## Decisions deliberately deferred

These are NOT in v1. They're listed so future work has a known landing
spot.

- **Computed values / expressions** (`60 * 60`, `BASE * 2`).
- **`use` imports** for cross-namespace ergonomics.
- **`newtype`** for nominal types (vs. structural `type`).
- **String interpolation** — constants only, low value.
- **Block comments** — kept off.
- **Line continuations** — only inside delimiters or after binary operators
  (rule defined now, enforced when expressions land).

## Decisions deliberately rejected

These were considered and ruled out, with reasons:

- **Significant whitespace.** Python and Haskell regret the indentation
  rules in some contexts. Newlines terminate; indentation is cosmetic.
- **Type inference at top level.** Go's `:=`/`var` and TS's `any` leakage
  are the cautionary tales.
- **Implicit numeric coercion.** `u32 X = 3.14` is an error.
- **Multiple comment styles.** One line, one doc, no block.
- **Sigils on names.** Plain identifiers.
- **Statement-terminating semicolons.** Newlines work and we never have a
  statement-following-expression case where ASI would trip us up.
- **`/regex/flags` literals.** Collides with division when expressions are
  added (the JS/Perl/Ruby footgun). Use a tagged string instead.
- **Multiple ways to spell the same thing.** The formatter picks one form.

## Appendix: comparison to v0 (TOML)

| v0 | v1 |
|----|----|
| `MAX = { type = "u32", value = 8 }` | `u32 MAX = 8` |
| `## doc` | `/// doc` |
| `__namespace__ = "core.time"` | `namespace core::time` |
| Inline enum: `{ type = "enum", variants = [...] }` | Top-level `enum Name { ... }` and use `Name` as a type |
| `.c.toml` | `.prim` |
| TOML grammar | Custom recursive-descent grammar |
