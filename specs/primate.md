# primate Requirements Specification

This document captures all requirements from RFC-0001 for tracey-based requirements tracking.

## Input Format

r[input.extension]
Input files must use the `.c.toml` extension.

r[input.namespace.directory]
Directory structure maps to namespaces (e.g., `math/linear.c.toml` becomes `math.linear`).

r[input.namespace.override]
Namespace can be overridden with `__namespace__ = "core.time"` in the file.

r[input.constant.format]
Constants are defined as `NAME = { type = "<type>", value = <value> }`.

r[input.constant.naming]
Constant names must be SCREAMING_SNAKE_CASE.

r[input.constant.unique]
Constant names must be unique per namespace.

r[input.doc.format]
Lines starting with `##` become doc comments.

r[input.doc.contiguous]
Only contiguous leading comments are captured; blank lines break grouping.

## Configuration

r[config.file]
Configuration is stored in `primate.toml` in the project root.

r[config.input.required]
The `input` field specifying the constants directory is required.

r[config.output.required]
At least one `[[output]]` section is required.

r[config.output.generator-or-plugin]
Exactly one of `generator` or `plugin` must be specified per output.

r[config.output.path]
Each output must specify a `path` for the output file or directory.

r[config.output.options]
Generator-specific settings are passed via the optional `options` table.

r[config.generator.builtin]
Built-in generators are: "typescript", "rust", "python".

r[config.cli.override]
CLI can override config with `-c other.toml` or inline `./constants -o ts:./out.ts`.

## Types - Scalars

r[type.scalar.i32]
The `i32` type represents a 32-bit signed integer.

r[type.scalar.i64]
The `i64` type represents a 64-bit signed integer.

r[type.scalar.u32]
The `u32` type represents a 32-bit unsigned integer.

r[type.scalar.u64]
The `u64` type represents a 64-bit unsigned integer.

r[type.scalar.f32]
The `f32` type represents a 32-bit floating point number.

r[type.scalar.f64]
The `f64` type represents a 64-bit floating point number.

r[type.scalar.bool]
The `bool` type represents a boolean value.

r[type.scalar.string]
The `string` type represents a UTF-8 string.

## Types - Duration

r[type.duration.format]
Duration values support formats: 150ms, 5s, 3m, 2h, 1h30m, 2d.

r[type.duration.internal]
Durations are stored internally as nanoseconds (u64).

r[type.duration.ts]
TypeScript outputs duration as milliseconds (number) or Temporal.Duration (configurable).

r[type.duration.rust]
Rust outputs duration as `std::time::Duration`.

r[type.duration.python]
Python outputs duration as `datetime.timedelta`.

## Types - Bytes

r[type.bytes.format]
Byte size values support formats: KB, MB, GB, TB, KiB, MiB, GiB, TiB.

r[type.bytes.internal]
Byte sizes are stored internally as bytes (u64).

r[type.bytes.output]
Byte sizes are output as integers (bytes).

r[type.bytes.ts-bigint]
TypeScript can optionally emit byte sizes as bigint for values exceeding Number.MAX_SAFE_INTEGER.

## Types - Containers

r[type.container.array]
`T[]` represents an array of type T.

r[type.container.nested-array]
`T[][]` represents a nested array.

r[type.container.map]
`map<K,V>` represents a key-value map.

r[type.container.tuple]
`tuple<T1,T2>` represents a fixed-size tuple.

r[type.container.optional]
`optional<T>` represents a nullable value.

r[type.container.empty]
Empty containers are allowed with explicit type annotation.

## Types - Enum

r[type.enum.simple]
Simple string enums use array syntax: `variants = ["pending", "active", "done"]`.

r[type.enum.string-backed]
Enums can have explicit string backing values: `variants = { Get = "GET" }`.

r[type.enum.int-backed]
Enums can have integer backing values: `variants = { Debug = 0, Info = 1 }`.

r[type.enum.variant-naming.rust]
Rust derives variant names as PascalCase from array-style enums.

r[type.enum.variant-naming.python]
Python derives variant names as SCREAMING_SNAKE_CASE from array-style enums.

r[type.enum.variant-naming.ts]
TypeScript uses values directly in union types for array-style enums.

r[type.enum.ts.string]
TypeScript outputs string enums as type union plus const array.

r[type.enum.ts.explicit-string]
TypeScript outputs explicit string enums as type union plus const object.

r[type.enum.ts.int]
TypeScript outputs integer enums as `enum` with numeric values.

r[type.enum.rust.string]
Rust outputs string enums with `as_str()` method returning the string value.

r[type.enum.rust.int]
Rust outputs integer enums with `#[repr(i32)]` attribute.

r[type.enum.python.string]
Python outputs string enums as `class Status(str, Enum)`.

r[type.enum.python.int]
Python outputs integer enums as `class LogLevel(IntEnum)`.

## Types - Struct

r[type.struct.infer]
Struct field types are inferred from values when not explicitly specified.

r[type.struct.explicit]
Struct field types can be explicitly specified via `fields` table.

## Types - Special

r[type.special.regex]
The `regex` type is validated at generation time per target language.

r[type.special.url]
The `url` type represents a validated URL string.

## Naming Conventions

r[naming.input]
Input constant names must be SCREAMING_SNAKE_CASE.

r[naming.rust]
Rust output uses SCREAMING_SNAKE_CASE.

r[naming.ts]
TypeScript output uses camelCase by default (configurable).

r[naming.python]
Python output uses SCREAMING_SNAKE_CASE.

r[naming.keyword-escape]
Reserved keywords get trailing underscore (e.g., `type_`).

## Code Generation - TypeScript

r[gen.ts.naming-option]
TypeScript supports `naming` option: "camelCase" (default) or "SCREAMING_SNAKE_CASE".

r[gen.ts.duration-option]
TypeScript supports `duration` option: "number" (ms, default) or "temporal".

r[gen.ts.bytes-option]
TypeScript supports `bytes` option: "number" (default) or "bigint".

r[gen.ts.enum-style-option]
TypeScript supports `enumStyle` option: "literal" (default), "const", or "enum".

## Code Generation - Rust

r[gen.rust.visibility-option]
Rust supports `visibility` option: "pub" (default), "pub(crate)", "pub(super)", or "".

r[gen.rust.derives]
Rust enums include `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`.

## Code Generation - Python

r[gen.python.typing-option]
Python supports `typing` option: "runtime" (default) or "stub" (emit .pyi only).

## Plugin System

r[plugin.executable]
Plugins are standalone executables.

r[plugin.resolve.path]
Plugin paths starting with `.` or `/` are used directly.

r[plugin.resolve.name]
Plugin names resolve to `primate-gen-<name>` in $PATH.

r[plugin.resolve.fallback]
Plugin names fallback to `~/.primate/plugins/<name>`.

r[plugin.protocol.stdin]
primate writes JSON CodeGenRequest to plugin's stdin.

r[plugin.protocol.stdout]
Plugin writes JSON CodeGenResponse to stdout.

r[plugin.request.version]
Request includes `version` field for protocol versioning.

r[plugin.request.output-path]
Request includes `outputPath` from config.

r[plugin.request.options]
Request includes `options` from config.

r[plugin.request.modules]
Request includes `modules` array with namespace, sourceFile, doc, and constants.

r[plugin.request.enums]
Request includes `enums` array with name, namespace, variants, and backingType.

r[plugin.response.files]
Response includes `files` array with path and content.

r[plugin.response.errors]
Response may include optional `errors` array.

r[plugin.exit.success]
Exit code 0 indicates success; read response from stdout.

r[plugin.exit.failure]
Exit code 1 indicates failure; response may contain errors array.

r[plugin.exit.crash]
Exit code >1 indicates crash; primate reports plugin failure.

r[plugin.version-check]
Plugins should validate request version and fail gracefully if unsupported.

r[plugin.atomic-write]
Files from plugin response are written atomically by primate.

## IR - Type Representation

r[ir.type.tagged]
Types are represented as tagged objects with `kind` field.

r[ir.type.array]
Array types include `element` field.

r[ir.type.map]
Map types include `key` and `value` fields.

r[ir.type.optional]
Optional types include `inner` field.

r[ir.type.tuple]
Tuple types include `elements` array.

r[ir.type.struct]
Struct types include `fields` map.

r[ir.value.duration]
Duration values are normalized to `{ "nanoseconds": N }`.

r[ir.value.bytes]
Byte size values are normalized to `{ "bytes": N }`.

r[ir.value.enum]
Enum values include `variant` and `value` fields.

r[ir.value.struct]
Struct values are plain objects with field values.

r[ir.value.scalar]
Scalar values are raw JSON values.

## Source Mapping

r[sourcemap.ts]
TypeScript uses standard `.js.map` files mapping to `.c.toml` sources.

r[sourcemap.lsp]
The `primate lsp` command provides Go to Definition for all languages.

r[sourcemap.json]
primate emits `primate.sourcemap.json` for editor extensions.

r[sourcemap.header]
Generated files include header comment with source file and "Do not edit" warning.

r[sourcemap.inline]
Languages without source map support include inline source references as comments.

r[sourcemap.format]
Sourcemap JSON includes version, and entries with symbol, source/output file and line info.

## Diagnostics - Errors

r[diag.error.unknown-type]
Unknown or invalid types produce errors.

r[diag.error.parse-failure]
Parse failures for duration, size, regex, url produce errors.

r[diag.error.duplicate-name]
Duplicate names in same namespace produce errors.

r[diag.error.invalid-identifier]
Invalid identifiers produce errors.

r[diag.error.type-mismatch]
Type/value mismatch produces errors.

r[diag.error.naming-convention]
Non-SCREAMING_SNAKE_CASE names produce errors.

r[diag.error.enum-variant]
Enum value not in declared variants produces errors.

r[diag.error.overflow]
Overflow during normalization produces errors.

r[diag.error.byte-overflow]
Byte size > u64 is rejected.

r[diag.error.circular-namespace]
Circular namespace references are rejected.

## Diagnostics - Warnings

r[diag.warn.unsafe-integer]
f64 > MAX_SAFE_INTEGER warns when TypeScript output is configured.

r[diag.warn.duration-precision]
Duration precision loss warns when converting to milliseconds.

r[diag.warn.keyword-collision]
Reserved keyword collision warns (auto-escaped, but flagged).

r[diag.warn.unused-namespace]
Unused namespace override produces warning.

r[diag.warn.target-aware]
Warnings are target-aware; JS number limits only warn if TypeScript is configured.

## Diagnostics - Info

r[diag.info.deprecated]
Deprecated syntax produces info-level hints.

r[diag.info.style]
Style suggestions produce info-level messages.

## CLI

r[cli.default-config]
`primate` without arguments uses `./primate.toml`.

r[cli.explicit-config]
`primate -c other.toml` uses explicit config file.

r[cli.inline]
`primate ./constants -o ts:./out.ts` allows inline configuration.

r[cli.check]
`primate check` validates without generating.

r[cli.check-watch]
`primate check --watch` provides continuous validation.

r[cli.generate-watch]
`primate generate --watch` watches for changes and regenerates outputs.

r[cli.lsp]
`primate lsp` starts the LSP server.

r[cli.check-format]
`primate check --format=json` outputs diagnostics as JSON.

## LSP Features

r[lsp.diagnostics]
LSP provides real-time diagnostics as you type.

r[lsp.goto-definition]
LSP provides Go to Definition for enum references and namespace overrides.

r[lsp.hover]
LSP provides hover documentation.

r[lsp.completion]
LSP provides completion for type names and enum variants.

r[lsp.rename]
LSP provides rename symbol across files in namespace.

## Security

r[security.no-execution]
No code execution, shell interpolation, environment access, or network access.

r[security.plugin-isolation]
Plugins run as separate processes with no special privileges.

r[security.no-env-passthrough]
primate does not pass environment variables or secrets to plugins.

## Pipeline

r[pipeline.discover]
Step 1: Discover .c.toml files.

r[pipeline.parse]
Step 2: Parse TOML (preserving comments).

r[pipeline.validate]
Step 3: Validate and normalize.

r[pipeline.build-ir]
Step 4: Build IR.

r[pipeline.sourcemap]
Step 5: Generate sourcemaps.

r[pipeline.builtin-emit]
Step 6: For built-in targets, emit output.

r[pipeline.plugin-invoke]
Step 7: For plugins, serialize IR, invoke plugin, collect response.
