# Enums and constants

Recipes for the most common shapes you'll write.

## A flat constants file

Start here. Many projects need a handful of named values and never
touch enums.

```primate
// constants/limits.prim

duration TIMEOUT     = 30s
u32      MAX_RETRIES = 5
u64      MAX_UPLOAD  = 100MiB
string   API_VERSION = "v3"
bool     STRICT_MODE = true
```

Generated TypeScript (`generated/constants/limits.ts`):

```typescript
export const timeout = 30_000 as const;
export const maxRetries = 5 as const;
export const maxUpload = 104_857_600 as const;
export const apiVersion = "v3" as const;
export const strictMode = true as const;
```

Plus a sibling `index.ts` re-exporting each namespace as a sub-object,
so consumers can write
`import { limits } from "./generated/constants"` and reach for
`limits.timeout`.

## A string-tagged enum

Use this when the enum is identified by name in serialized form (JSON
APIs, log fields, environment values). Variants have no explicit
`= value`.

```primate
// constants/job.prim

/// Operation status.
enum Status {
    Pending,
    Active,
    Done,
    Failed,
}
```

Generated TypeScript:

```typescript
/** Operation status. */
export type Status = "Pending" | "Active" | "Done" | "Failed";
export const Status = {
  Pending: "Pending",
  Active: "Active",
  Done: "Done",
  Failed: "Failed",
} as const;
```

The type union gives you compile-time exhaustiveness; the const object
gives you a runtime handle (`Status.Pending`) for non-literal callsites.
The `enumStyle` option on the TypeScript generator switches between
this default ("literal"), a `const` object only, or a real TS `enum`.

## An integer-backed enum

Use this when the enum lives on a wire format that wants a small
integer (telemetry, binary protocols, log levels).

```primate
// constants/log.prim

/// Severity, integer-backed for fast filtering.
enum LogLevel: u8 {
    Debug = 0,
    Info  = 1,
    Warn  = 2,
    Error = 3,
}
```

Generated TypeScript:

```typescript
/** Severity, integer-backed for fast filtering. */
export enum LogLevel {
  Debug = 0,
  Info = 1,
  Warn = 2,
  Error = 3,
}
```

(Rust generates `#[repr(i32)] pub enum`; Python generates `IntEnum`.)

## Per-variant docs

Doc comments attach to the next variant, just like for top-level
declarations:

```primate
enum LogLevel: u8 {
    /// Verbose logs intended for development.
    Debug = 0,

    /// Normal operational logs.
    Info  = 1,

    /// Something went wrong but the app continued.
    Warn  = 2,

    /// Something went wrong and the operation failed.
    Error = 3,
}
```

These docs land in the generated output (JSDoc on TypeScript variants,
docstring fields on Python enums, `///` on Rust variants).

## Using an enum-typed constant

Once the enum is declared, use its name as a type:

```primate
// constants/job.prim

enum Status {
    Pending,
    Active,
    Done,
}

Status DEFAULT_STATUS = Pending
```

Both bare (`Pending`) and qualified (`Status::Pending`) variant
references work. Cross-namespace, you'd write `job::Status::Pending`
or `use job::Status` first.

## Aliases for repetition

Type aliases reduce repetition when the same shape appears more than
once:

```primate
type Port = u32

Port HTTP_PORT  = 8080
Port HTTPS_PORT = 8443
Port ADMIN_PORT = 9999
```

The alias `Port` shows up in generated code as a named type
(`type Port = number;` in TypeScript), so call sites can talk about
"a port" rather than "a u32 that happens to be a port".

For aliases you don't want to surface as a named type, mark with
`@inline` — see [Attributes](../language/attributes.md).
