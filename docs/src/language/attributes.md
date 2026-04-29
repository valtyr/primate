# Attributes

Attributes are `@name` (or `@name(arg, arg, …)`) annotations that
attach to the declaration immediately following them. They control
how a declaration is treated by primate or by generators.

```primate
@inline
type Bytes32 = u64
```

Multiple attributes on one declaration stack on separate lines:

```primate
@inline
@some_plugin_attr
type Color = u32
```

Argument syntax: literals (`"strings"`, integers, booleans) or bare
identifiers, comma-separated, in `()`. The `@name(args)` form is
fully reserved at parse time, so plugins can introduce custom
attributes without forking the parser.

## `@inline`

Applies to **type aliases**. Suppresses the alias declaration in
generated output and inlines the underlying type at every use site.

```primate
@inline
type Bytes32 = u64

Bytes32 BIG_HASH = 100KiB
```

In generated code, `BIG_HASH` is typed as the underlying `u64`-style
representation in each target — no `Bytes32` alias appears in the
output.

Use `@inline` for *very* lightweight aliases where the name carries
no generator-side meaning. For semantic aliases that benefit from
showing up as a named type (e.g. `Port`), leave `@inline` off.

## Custom attributes

Attribute names that don't match a built-in emit a warning at parse
time, not an error. Plugins receive every attribute on every
declaration in their JSON request and decide how to interpret unknown
names.

```primate
@cdn(url = "https://cdn.example.com")
url ASSET_BASE = "https://example.com/assets"
```

`@cdn` isn't built-in; primate emits `unknown-attribute` (warning) and
passes `cdn(url="https://cdn.example.com")` through to any generator
that wants to act on it.

See [Writing a generator](../plugins/writing-a-generator.md) for how
to read attribute data inside a plugin.
