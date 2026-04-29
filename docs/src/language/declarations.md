# Declarations

There are four declaration kinds in a `.prim` file: `const`, `enum`,
`type` (alias), and `namespace` (one-liner override). This page covers
each.

## Constants

```primate
<type> <NAME> = <literal>
```

The type comes first; the name is `SCREAMING_SNAKE_CASE`; the value is
a literal of the declared type. One per line; no semicolons.

```primate
duration TIMEOUT     = 30s
u32      MAX_RETRIES = 5
u64      MAX_UPLOAD  = 100MiB
string   API_VERSION = "v3"
bool     STRICT_MODE = true
```

The type is mandatory — there's no inference at any level. This is a
deliberate choice; see [Overview](./overview.md) for the rationale.

Doc comments attach to the next declaration:

```primate
/// How long the gateway waits before bailing on a slow upstream.
///
/// Bumping this value also requires bumping the load-balancer's
/// idle-timeout — they must stay aligned.
duration UPSTREAM_TIMEOUT = 30s
```

`///` lines accumulate until the declaration; one blank line detaches
the doc block (it becomes a standalone `//` comment in the formatter
output).

### Alignment within groups

Consecutive declarations with no blank line between them form a *group*.
The formatter aligns the type, name, and `=` columns across the group:

```primate
duration TIMEOUT     = 30s
u32      MAX_RETRIES = 5
u64      MAX_UPLOAD  = 100MiB
```

A blank line breaks the group; doc comments don't.

## Enums

```primate
enum Name { Variant, … }
```

By default, variants are *string-tagged* — they serialize as their
PascalCase name in generators that distinguish.

```primate
/// Operation status.
enum Status {
    Pending,
    Active,
    Done,
}
```

For an integer-backed enum, add `: <int-type>` after the name:

```primate
enum LogLevel: u8 {
    Debug = 0,
    Info  = 1,
    Warn  = 2,
    Error = 3,
}
```

- Backing type must be an integer primitive (`i8`/`i16`/`i32`/`i64`/`u8`/`u16`/`u32`/`u64`).
- Variants without an explicit value get auto-assigned `0, 1, 2, …`.
- Enum bodies follow the same group-alignment rules as constants — the
  formatter aligns the `=` column across variants.

Trailing commas are accepted on the last variant; the formatter always
emits one for multi-line enum bodies.

## Type aliases

```primate
type Port           = u32
type ServiceConfig  = map<string, Port>
type Color          = tuple<u8, u8, u8>
```

Aliases are real first-class types. They're emitted as standalone type
declarations in the generated code so the alias's name shows up in
hover docs, IDE tooltips, and so on.

To suppress emission and inline the underlying type at use sites, mark
with `@inline`:

```primate
@inline
type Bytes32 = u64
```

Aliases participate in cross-file resolution exactly like enums:
sibling files in the same namespace see them by bare name; other
namespaces use a fully qualified path or a `use` import.

Alias chains (`type A = B`, `type B = C`) are resolved transitively at
IR time, so generated code never contains a chain.

## `namespace`

Each file belongs to one namespace. The default comes from the file's
path; the override looks like:

```primate
// constants/legacy/old_metrics.prim
namespace metrics::v1
```

Rules:

- One per file. Zero allowed (then the path-derived default is used).
- If present, must be the first non-comment item.
- Single line, no braces, `::` as separator.

You usually shouldn't need this. The path-derived default is the
recommended way to organize. Reach for an explicit `namespace` only
when:

- A file's path doesn't reflect where its declarations *should* live
  (the legacy-rename case above).
- Two files at different paths need to share a namespace and you don't
  want to move them.

## Order in the file

If a `namespace` override is present, it must come first. After that,
order is free — declarations don't have to come before they're used,
because resolution happens after the whole file (and project) is
parsed. The formatter doesn't reorder declarations.

`use` imports are an exception: they go at the top of the file, after
the optional `namespace` line, before any other declaration. The
formatter sorts and merges them; see [`use` statements](./use.md).
