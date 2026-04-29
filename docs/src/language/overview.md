# Language overview

A `.prim` file is a sequence of top-level items, separated by newlines.
There are six kinds of item:

| Item                 | Spelling                          | Notes                       |
|----------------------|-----------------------------------|-----------------------------|
| Constant             | `<type> <NAME> = <value>`         | One per line                |
| Enum                 | `enum Name { Variant, … }`        | Optionally integer-backed   |
| Type alias           | `type Name = <type>`              | Reusable type expression    |
| `use` import         | `use ns::Name` / `use ns::{A, B}` | Cross-namespace ergonomic   |
| `namespace` override | `namespace foo::bar`              | Escape hatch (see below)    |
| Comments             | `//`, `///`, `//!`                | Line, doc, file-doc         |

A real-world snippet:

```primate
/// Maximum upload size, enforced by the gateway.
u64 MAX_UPLOAD = 100MiB

/// Severity of a log line, integer-backed for fast filtering.
enum LogLevel: u8 {
    Debug = 0,
    Info  = 1,
    Warn  = 2,
    Error = 3,
}

type Port = u32

Port HTTP_PORT  = 8080
Port HTTPS_PORT = 8443
```

## Identifiers and naming

primate enforces case conventions. The formatter doesn't fix violations —
the parser surfaces a `naming-convention` diagnostic.

| Item            | Convention             | Example          |
|-----------------|------------------------|------------------|
| Constants       | `SCREAMING_SNAKE_CASE` | `MAX_UPLOAD`     |
| Enums           | `PascalCase`           | `LogLevel`       |
| Enum variants   | `PascalCase`           | `Warn`           |
| Type aliases    | `PascalCase`           | `Port`           |
| Namespaces      | `lower_snake_case`     | `core::time`     |

## Namespaces

Each `.prim` file belongs to exactly one namespace. By default the
namespace comes from the file's path relative to the project's `input`
directory:

```text
constants/                ← input
├── limits.prim           → namespace `limits`
├── time.prim             → namespace `time`
└── net/
    └── ports.prim        → namespace `net::ports`
```

This is the recommended way to organize. **Don't write `namespace foo`
at the top of every file** — let the directory layout do it. The
explicit form is an escape hatch when you want a file's contents to
live somewhere other than the path implies.

```primate
// constants/legacy/old_metrics.prim
namespace metrics::v1
// — overrides the path-derived `legacy::old_metrics`.
```

Files sharing a namespace share a flat scope: enums and aliases declared
in one are visible in all, by bare name. Cross-namespace references go
through fully qualified paths or [`use` statements](./use.md).

## Resolution rules

When you write a bare type name like `LogLevel`, primate looks for it in:

1. The current file.
2. Sibling files in the same namespace.
3. Names brought into scope by `use` statements.
4. (Otherwise: `unknown-type` error. Use a fully qualified path.)

## A few syntactic rules

- **No expressions in values.** Every constant is a literal of its
  declared type. No `60 * 60`, no `BASE * 2`.
- **No statement-terminating semicolons.** Newlines terminate.
- **Inside `()`, `[]`, `{}`, `<>` newlines are insignificant** —
  handy for wrapping long type expressions or value literals.
- **One comment style per role.** `//` line, `///` doc, `//!` file
  doc. No block comments.

Some features you might expect (expressions, `newtype` nominal
types, string interpolation) aren't in the language. See the
[roadmap](../roadmap.md) for what's under consideration.

## Where to read next

- [Declarations](./declarations.md) — `const`, `enum`, `type`,
  `namespace`.
- [Types](./types.md) — primitives, container constructors, fixed
  arrays.
- [Values](./values.md) — literals, including the magic trailing
  comma.
- [`use` statements](./use.md).
- [Attributes](./attributes.md) — `@inline` and the plugin extension story.
- [Formatting](./formatting.md) — what `primate fmt` does.
