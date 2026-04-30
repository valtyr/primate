<img src="./assets/logo.svg" alt="primate" width="120" />

<br/>

# primate

[![CI](https://github.com/valtyr/primate/actions/workflows/ci.yml/badge.svg)](https://github.com/valtyr/primate/actions/workflows/ci.yml)
[![Docs](https://github.com/valtyr/primate/actions/workflows/docs.yml/badge.svg)](https://valtyr.github.io/primate/)
[![Crates.io](https://img.shields.io/crates/v/primate.svg)](https://crates.io/crates/primate)
[![License](https://img.shields.io/crates/l/primate.svg)](./LICENSE)

`primate` is a small DSL and code generator for cross-language constants.
You declare your shared values once, and it spits out idiomatic, typed
Rust, TypeScript, and Python.

### Setup

1. Install:

   ```sh
   cargo install primate --locked
   ```

2. Drop a `primate.toml` at the project root pointing at a directory of
   `.prim` files and listing your targets:

   ```toml
   input = "constants"

   [[output]]
   generator = "typescript"
   path      = "web/src/generated/constants/"

   [[output]]
   generator = "rust"
   path      = "src/generated/constants.rs"
   ```

3. Write some constants:

   ```primate
   // constants/limits.prim

   /// Maximum upload size for a single request.
   u64 MAX_UPLOAD_SIZE = 100MiB

   /// Severity level. Integer-backed for fast filtering.
   enum LogLevel: u8 {
       Debug = 0,
       Info  = 1,
       Warn  = 2,
       Error = 3,
   }

   LogLevel DEFAULT_LEVEL = Info
   ```

4. Run `primate build` and you'll get something like this:

   ```rust
   // src/generated/constants.rs
   pub mod limits {
       pub const MAX_UPLOAD_SIZE: u64 = 104857600;

       #[derive(Debug, Clone, Copy, PartialEq, Eq)]
       #[repr(i32)]
       pub enum LogLevel { Debug = 0, Info = 1, Warn = 2, Error = 3 }

       pub const DEFAULT_LEVEL: LogLevel = LogLevel::Info;
   }
   ```

   ```typescript
   // web/src/generated/constants/limits.ts
   export enum LogLevel { Debug = 0, Info = 1, Warn = 2, Error = 3 }
   export const maxUploadSize = 104857600 as const;
   export const defaultLevel = LogLevel.Info as const;
   ```

   Add a `[[output]]` block for `python` and you'll get a parallel
   package with `IntEnum` and `timedelta` in the right places.

### Motivation

If you ship code to two or more language ecosystems, you've probably
written the same constants more than once. The Node service has its
own `MAX_UPLOAD_SIZE`, the Rust worker has another, the Python script
has a third. They drift, one ends up wrong, and the bug shows up at
2am.

The usual fixes are awkward. A JSON config file gives up types and
docs. A shared package only works when the languages can interop.
Manually keeping things in sync works exactly until it doesn't.

`primate` takes a different angle: declare constants in one place,
generate them in each target's idioms. Durations end up as
`std::time::Duration` in Rust, `Temporal.Duration` (or milliseconds)
in TypeScript, `timedelta` in Python. Integer-backed enums become
`#[repr]`, TS `enum`, and `IntEnum`. Doc comments follow the values
to every callsite. Suffixed numeric literals — `30s`, `100MiB`, `5%`,
`1w` — are bounds-checked at parse time, then become normal numbers
in the generated output.

A canonical formatter (`primate fmt`) and an LSP server with
diagnostics, hover, go-to-definition, find-references, and contextual
completion ship with the binary. If you need a target the built-ins
don't cover, there's a stdin/stdout plugin protocol.

### Editor support

- **[Zed](./editors/zed)** — install as a dev extension.
- **[VS Code](./editors/vscode)** — install the extension locally.
- **[Vim](./editors/vim)** — syntax + ftdetect files.

### Documentation

Full docs at [**valtyr.github.io/primate**](https://valtyr.github.io/primate/),
or build locally:

```sh
cd docs && mdbook serve --open
```

### License

MIT — see [LICENSE](./LICENSE).
