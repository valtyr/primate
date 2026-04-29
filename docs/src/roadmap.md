# Roadmap

Features under consideration. No timelines — items land if a real use
case shows up; otherwise they sit here. The point of this page is to
make explicit what's *not* in the language and *why*, without sprinkling
those notes throughout the reference.

## Likely

- **`@deprecated(message?)` attribute.** Marks a constant as
  deprecated; generators emit a target-language deprecation marker
  (Rust `#[deprecated]`, JSDoc `@deprecated`, etc.). Cut from v1
  because it adds surface without solving a problem most projects
  hit; will revisit if real demand shows up.
- **`use a::b::C as D` rename imports.** Rust-style; useful for
  resolving collisions between same-named imports. Lands when the
  collision case becomes painful in real projects.
- **Configurable formatter column budget.** Currently fixed at 100.
  Easy to add via `primate.toml` if anyone wants a different number.
- **`/regex/flags` regex literal syntax.** Considered and deferred in
  RFC 0004 in favor of the same string-with-`(?i)` form that Rust
  and Python use. If inline-flag ergonomics become a recurring
  papercut, revisit.

## Maybe

- **`newtype` for nominal types.** `type Port = u32` is *structural*:
  `Port` and `u32` are interchangeable. A `newtype Port = u32` would
  make them distinct, catching "I passed a `Count` where I meant a
  `Port`" in target-language code. The Rust story is clean
  (`pub struct Port(pub u32);`); the TypeScript story (branded types)
  is awkward; the Python story (`NewType`) is type-checker-only.
  Lands if a real project needs the discipline.
- **Other unit-suffix categories.** Today only byte-size suffixes are
  recognized on integer literals. `%` (percent), `Hz` (frequency),
  `m`/`cm`/`km` (length), currency codes — each could exist as a
  parsing affordance. Picking one without a principled criterion is
  arbitrary, so we don't.

## Probably not

- **Glob imports** (`use a::*`). Mass-import is a name-shadowing
  hazard; explicit `use` lines make collisions and provenance clear.
- **Block comments** (`/* */`). One line, one doc, one file-doc.
- **Significant whitespace.** Newlines terminate; indentation is
  cosmetic.
- **Sigils on names** (`$X`, `@X`). Plain identifiers.

## Out of scope

- **Expressions and arithmetic.** primate is a constants language.
  If you need to compute a value, do it in your build script and
  paste the result.
- **String interpolation.** Constants only; low value.
- **Statement-terminating semicolons.** Newlines work and there's
  no expression context where ASI-style ambiguity could appear.
