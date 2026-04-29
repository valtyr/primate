# Cross-namespace types

Real projects have more than one file. This page shows how to
organize types across namespaces and reference them from elsewhere.

## File layout drives namespaces

The recommended pattern: let the directory layout determine the
namespace. **Don't write `namespace foo` at the top of files.**

```text
constants/                     ← input
├── net/
│   ├── limits.prim            → namespace `net::limits`
│   └── headers.prim           → namespace `net::headers`
├── log/
│   └── levels.prim            → namespace `log::levels`
└── jobs.prim                  → namespace `jobs`
```

Files with the same parent share a namespace. Sibling files in
`net/` see each other's enums and aliases by bare name.

## Cross-namespace by qualified path

Reference a type from another namespace by its fully qualified path:

```primate
// constants/jobs.prim

log::levels::LogLevel DEFAULT_LEVEL = Info
```

Generated TypeScript imports the type from the right namespace
automatically.

## Or with `use` for ergonomics

If you reference a name often, bring it into scope with `use`:

```primate
// constants/jobs.prim

use log::levels::LogLevel
use net::limits::{Port, IP}

LogLevel DEFAULT_LEVEL  = Info
Port     COORDINATOR    = 9000
IP       DEFAULT_BIND   = "0.0.0.0"
```

`use` is purely an ergonomic — it has no effect on generated code.
See [`use` statements](../language/use.md) for the rules.

## A shared types file

Group types that multiple namespaces need into a `core/types.prim`
or similar:

```primate
// constants/core/types.prim

/// Used everywhere a network port is named.
type Port = u32

/// IPv4 or IPv6 address.
type IP = string

/// Severity, integer-backed for fast filtering.
enum LogLevel: u8 {
    Debug = 0,
    Info  = 1,
    Warn  = 2,
    Error = 3,
}
```

Other files import via qualified path or `use`:

```primate
// constants/services/api.prim

use core::types::{Port, LogLevel}

Port     API_PORT      = 8080
LogLevel API_LOG_LEVEL = Info
```

## When to override the namespace

The escape hatch (`namespace foo::bar` at the top of a file) is for
the rare case where path-derived doesn't fit:

```primate
// constants/legacy/old_metrics.prim
namespace metrics::v1
// — overrides the path-derived `legacy::old_metrics` so we can keep
//   serving these on the existing `metrics::v1` API.
```

Use it sparingly. If you find yourself overriding more than once or
twice, that's a signal the directory layout doesn't reflect your
intended organization — move the files instead.

## Same name in two namespaces

primate allows the same type or constant name to exist in different
namespaces. Within a single namespace, duplicates are an error.

```primate
// constants/net/limits.prim
type Port = u32

// constants/audio/limits.prim
type Port = u8         // OK — different namespace
```

If you `use` both at once into a third file, that's an
`import-collision` error:

```primate
use net::limits::Port
use audio::limits::Port  // ✗ `Port` is already imported from `net::limits`
```

Either `use` only one, or qualify both at the call site.

## Where this matters in generated code

Each language preserves your namespace structure idiomatically:

- **TypeScript** — primate emits one `.ts` file per namespace (plus
  an `index.ts` re-exporting each one). Cross-namespace references
  become real ES `import` statements at the top of each file.
- **Rust** — primate emits a single `.rs` file with one
  `pub mod <ns> { ... }` per namespace. Cross-namespace references
  become `super::<other>::X`.
- **Python** — primate emits a package directory with one `.py` per
  namespace and an `__init__.py`. Cross-namespace references become
  `from .<other> import X`.

So if `limits.prim` references `LogLevel` from `logging`, you get the
right import or path at the consumer site for free.
