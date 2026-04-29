# RFC 0003 — Tuple value syntax, fixed-size arrays, `use`, line wrapping

Status: Accepted
Date: 2026-04-28
Extends: RFC 0002

## Summary

Five changes to the primate source language and formatter, motivated by
real ergonomics issues hit during early use:

1. Tuple **values** are written with `[...]` instead of `(...)`. Tuple
   **types** keep `tuple<...>`.
2. Add a fixed-size array type `array<T, N>` distinct from variable-length
   `array<T>`.
3. Add `use` statements (single and brace forms) for ergonomic
   cross-namespace references.
4. Newlines are insignificant inside `<>`, `[]`, `()`, `{}`. The formatter
   wraps long logical lines automatically.
5. The formatter sorts, merges, and simplifies `use` statements.

These changes were deferred in RFC 0002 (`use`, line continuations) or fell
out of practical use (the tuple-vs-array confusion at value sites, the
absence of a clean way to spell fixed-shape data like RGB or matrices).

## 1. Tuple values use `[...]`

### Today

```primate
type Color = tuple<u8, u8, u8>
Color BLACK = (0, 0, 0)
```

### Proposed

```primate
type Color = tuple<u8, u8, u8>
Color BLACK = [0, 0, 0]
```

### Rationale

- Visual consistency: ordered collections all use `[]`. Matches
  TypeScript's tuple syntax.
- Frees `(...)` for future expression grouping. Once expressions land
  (`60 * (60 + 1)`), grouping is the natural use of `()`. If `()` is
  taken for tuples, expressions get awkward.
- Python's `(1,)` single-tuple footgun is sidestepped — there's no
  ambiguity between `[x]` (tuple of one) and `(x)` (parenthesized
  expression).

### Disambiguation

The parser cannot distinguish array literals from tuple literals at the
value site without LHS type info. Both lex/parse as `[...]`.

The AST gains a single variant:

```rust
ValueExprKind::List(Vec<ValueExpr>)
```

The lower pass resolves `List` to either `Array` or `Tuple` based on the
declared type of the constant. A `List` against an unknown type emits an
unknown-type diagnostic (the `List` itself is well-formed).

Map literals stay `{...}` — distinct shape, no overlap.

### Migration

Existing `(...)` tuple values are a parse error after this lands. The
formatter will rewrite them in place; users who never run the formatter
need to update by hand. This is a hard break, but it's v1; we don't have
external users yet.

## 2. Fixed-size arrays: `array<T, N>`

### Why both tuple and fixed-size array?

The two collapse to the same type in TypeScript, Python, and JSON. They
*don't* collapse in Rust:

| primate                | Rust generated   |
|------------------------|------------------|
| `tuple<u8, u8, u8>`    | `(u8, u8, u8)`   |
| `array<u8, 3>`         | `[u8; 3]`        |

The Rust difference is real: indexed vs positional access, iterable vs
not. For mathematical/repeated data (matrices, RGB pixels, fixed-shape
buffers), the array form is what users want.

We could collapse them at the source level and let code-gen pick — but
that loses user intent. A `tuple<u8, u8, u8>` declared as a (Y, Cb, Cr)
chroma triple is *structurally* a tuple even though all three slots are
`u8`. Forcing it into a fixed array because of type uniformity would
flatten that distinction.

### Form

```primate
type Pixel  = array<u8, 3>
type Matrix = array<Pixel, 3>     // i.e. array<array<u8, 3>, 3>

Pixel  WHITE    = [255, 255, 255]
Matrix IDENTITY = [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
]
```

`N` is a non-negative integer literal. No expressions in v1; underscores
in digit separators are allowed (consistent with all other integer
literals in the language).

### Variable-length vs. fixed-length

- `array<T>` — variable-length. Generated as growable list/vec in target
  languages.
- `array<T, N>` — fixed-length. Generated as language-native fixed-size
  array in Rust; as a homogeneous tuple in TypeScript/Python.

The IR distinguishes them with a separate variant or a `length:
Option<u32>` field on the existing `Array` variant; either is fine for
the implementation.

### Validation

- Length mismatch is a hard error: `array<u8, 3> X = [1, 2]` →
  `length-mismatch` diagnostic.
- Element-type mismatches are caught the same way as for `array<T>`.

### Future sugar

`[T; N]` as a shorthand for `array<T, N>` is desirable (matches Rust)
but **not in this RFC**. It conflicts visually with the existing `T[]`
sugar for `array<T>` and warrants its own design pass.

## 3. `use` statements

### Form

```primate
use core::types::LogLevel
use net::limits::{Port, IP, CIDR}
```

Two forms: single path, or path with brace group.

- Path uses `::` consistent with namespace declarations.
- The leaf in the single form, and each item in the brace form, is the
  *bare* name to import.
- Braces accept a trailing comma.
- Renames (`use a::b::C as D`) are **not** in this RFC. Reserved for a
  future additive change.
- Glob imports (`use a::b::*`) are **not** in this RFC. Discouraged in
  general; we'll resist adding them unless real demand emerges.

### Placement

`use` statements appear after the optional `namespace` declaration and
before the first non-`use` declaration (consistent with most languages).
Out-of-order `use` statements are a parse error.

### Resolution (extends RFC 0002 §"Resolution rules")

The resolution order in a file becomes:

1. Same file: top-level enums and aliases.
2. Same namespace, sibling files.
3. Names brought into scope by `use` statements in this file.
4. Cross-namespace by qualified path (still allowed).

### Diagnostics

- `unresolved-import`: `use a::b::C` where `a::b::C` doesn't exist.
- `import-collision`: a `use` brings in a name that collides with a
  same-namespace local declaration, or with another `use`.
- `unused-import`: warning, not error. The fix is to delete the line.

### Effect on code-gen

`use` is a source-only ergonomic. The IR records imports per file but
generators don't see them — they only see fully resolved type
references in `EnumDef`/`TypeAliasDef`/`Constant.typ`.

## 4. Newlines inside delimiters + formatter wrapping

RFC 0002 already commits to the rule:

> "expressions only span lines inside `(`, `[`, `{` or trailing
> operators (Python's rule, not JS's ASI)."

The parser doesn't enforce it yet. This RFC makes it normative for the
existing delimiters (no expressions yet, so the trailing-operator part
is still future work).

### Parser change

In every loop reading comma-separated items inside `<>`, `[]`, `()`,
`{}`, skip `Newline` and `BlankLine` tokens. This applies to:

- Tuple type contents (`tuple<A, B, C>`)
- Map type contents (`map<K, V>`)
- Fixed/variable array type contents (`array<T>`, `array<T, N>`)
- Optional type contents (`optional<T>`)
- List literal contents (`[1, 2, 3]`)
- Map literal contents (`{"k": v}`)
- Enum body contents (`{ Variant, ... }`)
- Attribute argument lists (`@name(arg, arg)`)
- Brace-form `use` items (`use a::b::{C, D}`)

Outside delimiters, newlines still terminate declarations.

### Formatter change

When a logical line exceeds a column budget, wrap at the **outermost**
wrap-eligible delimiter:

- One item per line.
- Trailing comma on the last item.
- Inner contents indented +4 from the line that opened the delimiter.

```primate
// Before
type ServiceConfig = map<string, tuple<duration, bytes, optional<url>, regex>>

// After
type ServiceConfig = map<
    string,
    tuple<duration, bytes, optional<url>, regex>,
>
```

The default column budget is 100. Configurable via `primate.toml` later
(out of scope for this RFC; default is fine for now).

If the *innermost* delimiter alone is enough to fit within the budget,
wrap there instead. Choose the shallowest depth that fits.

Wrapping never breaks a string literal across lines — strings are atomic
to the formatter.

### Multi-line value literals

The same rule applies to value literals:

```primate
Matrix IDENTITY = [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
]
```

### Magic trailing comma

A *trailing comma* on the last element of a collection literal is a signal
to the formatter: keep this multi-line, even if it would fit on one line.
This pattern was popularized by Prettier and now appears in dart, ruff,
dprint, and Black-for-some-cases.

Rules for value-side collection literals (`[...]` arrays/tuples and
`{...}` maps):

| Last element                         | Single-line fits? | Result      |
|--------------------------------------|-------------------|-------------|
| No trailing comma                    | yes               | single-line |
| No trailing comma                    | no                | multi-line  |
| Trailing comma                       | (irrelevant)      | multi-line  |

Multi-line output always *adds* a trailing comma; the formatter is
idempotent under this rule.

Example. The compact form fits on one line, so it stays compact:

```primate
Mat2 SMALL = [[1, 0], [0, 1]]
```

The same shape with a trailing comma stays expanded — useful for
matrices and lookup tables where the rows-as-rows layout is the point:

```primate
Mat3 IDENTITY = [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
]
```

The AST records the trailing-comma bit per collection literal so the
formatter can act on it.

Trailing commas on type-side generic args (`tuple<A, B,>`,
`map<K, V,>`) remain accepted but carry no formatter meaning. Types
are usually short and the budget rule alone is sufficient.

## 5. Formatter: sort, merge, simplify `use` statements

The block of `use` statements at the top of a file is normalized:

### Simplify

A single-item brace group collapses to bare form:

```
use a::b::{Port}     →   use a::b::Port
```

### Merge

Multiple `use` statements with the same path collapse into one:

```
use a::b::{Port, IP}
use a::b::CIDR
                     →   use a::b::{CIDR, IP, Port}
```

### Sort

After merging:

- Top-level: sort `use` lines by full path lexicographically.
- Inside braces: sort items lexicographically.

A leading comment attached to a `use` line (`/// note` or `// note`)
*pins* that line — sort/merge happens within contiguous comment-free
runs. This avoids silently moving comments away from the lines they
annotate.

### Examples

Input:
```primate
use core::types::LogLevel
use net::limits::{Port}
use net::limits::{IP, CIDR}
use core::types::Status
```

Output:
```primate
use core::types::{LogLevel, Status}
use net::limits::{CIDR, IP, Port}
```

## Decisions deliberately deferred

- **Renames in `use`:** `use a::b::C as D`.
- **Glob imports:** `use a::b::*`.
- **`[T; N]` sugar** for `array<T, N>`.
- **Repetition sugar inside tuples** (`tuple<u8; 3>`). RFC 0002 already
  supports the long form; users with all-same-type fixed shapes should
  reach for `array<T, N>` instead.
- **Configurable column budget** for the wrapper.

## Decisions deliberately rejected

- **`tuple<...>` collapsed into `array<T, N>` in the source language.**
  Considered. Lost the heterogeneous vs homogeneous distinction at the
  source level. Rust users care about `(A, B, C)` vs `[T; N]`.
- **`{` for tuple values to mirror Rust.** Visually clashes with map
  literals.
- **Hard column limit.** Soft (i.e. wrap when over) is friendlier than
  rejecting at parse time. The lexer/parser don't know about column
  budgets.

## Implementation order

Independently shippable in this order:

1. **Perf cache** for `parse_workspace` (per-file content-hash cache,
   debounced diagnostics). Not part of this RFC syntactically; called
   out because it's the highest-value change for current users.
2. Parser leniency: newlines inside delimiters.
3. `use` parsing + lowering.
4. Fixed-size `array<T, N>`.
5. Tuple values use `[...]`. (List → Array/Tuple disambiguation in
   lowering.)
6. Formatter: line wrapping.
7. Formatter: sort/merge/simplify `use`.
