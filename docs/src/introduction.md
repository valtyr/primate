# Introduction

primate compiles one set of constants into idiomatic, typed code for
Rust, TypeScript, and Python.

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

`primate build` reads that file and produces a typed module per target,
preserving your namespace structure. The TypeScript output:

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

The Rust and Python equivalents land in their idiomatic shapes —
`std::time::Duration` in Rust, `timedelta` in Python, `IntEnum` for
the integer-backed enum. See [Getting started](./getting-started.md)
for the full pipeline.

## Why primate

If you ship to two or more language ecosystems, you've probably written
the same `MAX_UPLOAD_SIZE` more than once. The Node service has its own
copy, the Rust worker has another, the Python script has a third. They
drift. One ends up wrong. The bug shows up at 2am.

The usual fixes are awkward. A JSON config file gives up types and
docs. A shared package only works when the languages can interop.
Manually keeping things in sync works exactly until it doesn't.

primate's angle is to declare constants once and generate them in each
target's idioms — values bounds-checked at parse time, doc comments
following the values to every callsite, real cross-namespace imports
in the generated code. The DSL is type-first and declaration-only;
there are no expressions, no computed values, no scope for
arithmetic. The small surface is deliberate.

## What you'll find here

- **[Getting started](./getting-started.md)** — install, first
  `.prim` file, generated output.
- **Language** — the full `.prim` syntax: declarations, types, values,
  `use`, attributes, formatting.
- **CLI** — `primate build`, `primate fmt`, `primate lsp`.
- **Plugins** — write a generator for a target the built-ins don't
  cover.
- **Editors** — VS Code, Zed, and Vim setup.
- **Cookbook** — recipes for common shapes: matrices, enums with
  metadata, cross-namespace organization.
- **Reference** — full grammar, every diagnostic code, the changelog.

## Status

primate is at v0.1 — usable, with the Rust, TypeScript, and Python
generators complete and an LSP server that works in real editors.
Expect occasional churn as the language settles; the
[roadmap](./roadmap.md) lists what's under consideration and what's
explicitly out of scope.
