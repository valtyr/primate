# Changelog

Notable changes per release. Pre-1.0; expect some churn.

## Unreleased

### Added

- **`use` statements** for cross-namespace ergonomic imports
  ([RFC 0003 §3](https://github.com/valtyr/primate/blob/main/rfc/0003-tuples-arrays-use-wrapping.md)).
  Single (`use a::b::C`) and brace (`use a::b::{C, D}`) forms.
- **Fixed-size arrays:** `array<T, N>` is a distinct IR type. Rust
  emits `[T; N]`; TypeScript and Python emit homogeneous tuples.
  Length-mismatch is a hard error.
- **Magic trailing comma** in value-side `[…]` and `{…}` literals:
  triggers multi-line formatting even when single-line would fit.
- **Workspace-wide LSP**: diagnostics resolve cross-file types,
  hover/goto-def/find-references all follow `use` imports, with a
  per-file content-hash cache so unchanged files aren't re-parsed.
- **Newlines insignificant inside delimiters** (`<>`, `[]`, `(`, `{}`).
  Long types and value literals can wrap freely.
- **Formatter line-wrap** at column 100 for long values.
- **Formatter `use` normalization**: simplifies single-brace, merges
  same-path, sorts top-level and inside braces.

### Changed

- **Tuple values use `[…]`** instead of `(…)`. Old `(…)` form is
  rejected with a clear migration message.
- **Project renamed** from `cconstt` to `primate`. File extension
  changed from `.const` to `.prim`.
- **Integer IR widened** from `i64` to `i128`. `u64::MAX` is
  representable; bounds checking applies per declared type.
- **`bytes` type dropped.** Byte-size suffixes (`B`, `KiB`, `MiB`,
  `GiB`, …) are now sugar on integer literals. Existing
  `bytes` declarations are a parse error.

### Removed

- **`@only(...)` and `@deprecated(...)` attributes.** Only `@inline`
  remains as a built-in; plugins can still read arbitrary
  attributes.

### LSP

- Hover with doc comments for types, including imported ones.
- Go-to-definition for types in any namespace.
- Find references across the workspace, following `use` imports.
- Per-file content-hash parse cache: only the changed file gets
  re-lexed/re-parsed on each keystroke.
- Completion includes types from the workspace, namespace-qualified
  when needed; built-ins and current-file types take priority.

## 0.1.0 — initial DSL

The `.prim` syntax landed under
[RFC 0002](https://github.com/valtyr/primate/blob/main/rfc/0002-primate-syntax.md):
type-first declarations, no inference; primitive types incl. `duration`,
`bytes`, `regex`, `url`; container constructors; enums (string-tagged
and integer-backed); `type` aliases; `@deprecated`/`@only`/`@inline`
attributes; canonical formatter.

Built-in generators: Rust, TypeScript, Python.
