# Introduction

**primate** compiles one set of constants into idiomatic, typed code for
Rust, TypeScript, and Python.

You write your constants once — durations, byte sizes, enums, type aliases,
anything else — in `.prim` files. primate produces well-formed, statically
typed source for each target language. The same value is reachable by name
from any of them, and stays in sync as you edit.

```primate
// constants/limits.prim

duration TIMEOUT     = 30s
u32      MAX_RETRIES = 5
u64      MAX_UPLOAD  = 100MiB

/// Severity, integer-backed for log filtering.
enum LogLevel: u8 {
    Debug = 0,
    Info  = 1,
    Warn  = 2,
    Error = 3,
}
```

Run `primate build` and you get one file (or directory) per target,
preserving your namespace structure. The TypeScript output looks like:

```typescript
// generated/constants/limits.ts
/** Severity, integer-backed for log filtering. */
export enum LogLevel {
  Debug = 0,
  Info = 1,
  Warn = 2,
  Error = 3,
}

export const timeout = 30_000 as const;            // milliseconds
export const maxRetries = 5 as const;
export const maxUpload = 104_857_600 as const;
```

The Rust and Python outputs are equivalent in shape — `std::time::Duration`
in Rust, `timedelta` in Python, integer-backed enums in both. See
[Getting started](./getting-started.md) for the full pipeline.

## Why primate

The problem primate solves is small but persistent: a single project that
spans languages — a Rust backend, a TypeScript frontend, a Python migration
script — needs to agree on its constants. Service ports, log levels, byte
limits, regex patterns. Re-typing them in every language produces three
copies that drift, three sets of type representations to keep aligned, and
three review surfaces for the same change.

primate lets the source of truth live in one place, with one review, and
have generated output in the idiomatic shape each target expects:
`std::time::Duration` in Rust, milliseconds-as-`number` in TypeScript (or
`Temporal.Duration` if you ask), `timedelta` in Python.

The DSL is type-first and declaration-only. There are no expressions,
arithmetic, or computed values in the language — just declarations of
constants, enums, and type aliases. That keeps the surface small and
predictable.

## What you'll find in this book

- **[Getting started](./getting-started.md)** — install primate, write your
  first `.prim` file, generate output.
- **Language** — the `.prim` syntax: declarations, types, values,
  `use`, attributes, formatting.
- **CLI** — `primate build`, `primate fmt`, `primate lsp`.
- **Plugins** — write your own code generator for a target the built-ins
  don't cover.
- **Editors** — Zed, VS Code, and Vim setup.
- **Cookbook** — recipes for common shapes: matrices, enums with metadata,
  cross-namespace organization, platform-specific output.
- **Reference** — full grammar, every diagnostic code, and the changelog.

## Status

primate is a young project. The Rust, TypeScript, and Python generators
work end-to-end. There's an LSP server with editor integrations for
Zed, VS Code, and Vim.

The [roadmap](./roadmap.md) lists features under consideration.
