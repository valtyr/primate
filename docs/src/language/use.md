# `use` statements

`use` brings a name from another namespace into the current file's
scope, so you can reference it by bare name instead of by fully
qualified path.

## Two forms

Single:

```primate
use net::limits::Port

Port HTTP_PORT = 8080
```

Brace group:

```primate
use net::limits::{Port, IP, CIDR}

Port HTTP_PORT = 8080
IP   FALLBACK  = "10.0.0.1"
```

Both forms are equivalent for resolution; brace is just shorthand for
multiple `use` lines from the same path.

## Placement

`use` statements appear after the optional `namespace` line and before
any other declarations:

```primate
namespace api::v2          // optional override

use net::limits::{Port, IP}
use core::types::LogLevel

Port      HTTP_PORT     = 8080
LogLevel  DEFAULT_LEVEL = Info
```

Out-of-order `use` statements (e.g. interleaved with constants) are a
parse error.

## Resolution

When you write a bare type name like `Port` in a file with
`use net::limits::Port`, primate resolves it as if you'd written
`net::limits::Port`. The resolution order in a file is:

1. The current file.
2. Sibling files in the same namespace.
3. Names brought into scope by `use` statements.
4. (Otherwise: `unknown-type` error. Use a fully qualified path.)

## Diagnostics

| Code                | Triggered when                                       |
|---------------------|------------------------------------------------------|
| `unresolved-import` | `use a::b::C` where `a::b::C` doesn't exist.         |
| `import-collision`  | A `use` brings in a name that collides with a same-namespace declaration, or with another `use`. |


## Formatter behavior

`primate fmt` normalizes the `use` block at the top of the file. The
rules:

- A single-item brace group collapses: `use a::b::{X}` → `use a::b::X`.
- Multiple `use` statements with the same path merge:
  `use a::b::X` + `use a::b::{Y, Z}` → `use a::b::{X, Y, Z}`.
- Top-level `use` lines sort lexicographically by path.
- Items inside a brace group sort lexicographically.

Example:

```primate
// Before
use core::types::LogLevel
use net::limits::{Port}
use net::limits::{IP, CIDR}
use core::types::Status

// After `primate fmt`
use core::types::{LogLevel, Status}
use net::limits::{CIDR, IP, Port}
```

A `///` or `//` comment immediately above a `use` line *pins* it: sort
and merge happen within contiguous comment-free runs only. This avoids
silently moving a comment away from the line it annotates.

## Effect on generated code

`use` is a source-only ergonomic. Generators don't see imports — they
work off fully resolved IR types. So `use net::Port` followed by
`Port HTTP_PORT = 8080` generates the same output as
`net::Port HTTP_PORT = 8080`.
