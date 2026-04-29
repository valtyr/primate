# Changelog

All notable changes to **primate** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1](https://github.com/valtyr/primate/compare/v0.1.0...v0.1.1) - 2026-04-29

### Other

- drop Open VSX publish for now
- auto-sync editor manifest versions on release PRs
- wire release automation (release-plz + extension publishes)
- Document type fidelity across generators
- rustfmt

## [0.1.0] — 2026-04-29

Initial public release.

### Added

- **Language**: type-first declarations, primitive types (`i8`–`i64`,
  `u8`–`u64`, `f32`/`f64`, `bool`, `string`, `duration`, `regex`, `url`),
  containers (`array<T>`, `array<T, N>`, `optional<T>`, `tuple<...>`,
  `map<K, V>`), `type` aliases, integer- and string-tagged `enum`s.
- **Unit suffixes** on numeric literals: durations (`ns`, `us`, `ms`, `s`,
  `min`/`m`, `h`, `d`, `w`), byte sizes on integers (`B`, `KB`/`MB`/`GB`/`TB`,
  `KiB`/`MiB`/`GiB`/`TiB`), and percentage on floats (`%`). Bounds-checked
  against the declared type.
- **Cross-namespace types.** `use` imports, qualified paths
  (`logging::LogLevel`), and namespace overrides.
- **Generators** for Rust (`pub mod` per namespace), TypeScript (per-
  namespace `.ts` files + `index.ts`), and Python (per-namespace `.py` files
  + `__init__.py`). Cross-namespace references emit the right idiomatic
  import or path qualifier in each target.
- **`primate fmt`** — single canonical formatter; no flags.
- **`primate lsp`** — LSP server with diagnostics, hover, go-to-definition,
  find-references, and contextual completion (enum variants, unit suffixes).
- **Plugin protocol** — JSON over stdin/stdout for third-party generators.
- **Editor integrations** — Zed and VS Code; vim syntax/ftdetect files.
- **Documentation** — mdBook in `docs/`.
