# `primate init`

Scaffolds a `primate.toml` in the current working directory. Walks you
through the source directory, which target languages to generate, the
output path for each, and any external plugins.

```bash
primate init
```

primate is meant to drop into existing repos, so `init` only creates
the config — no `constants/` directory, no starter `.prim`, no
`AGENTS.md`, no `.gitignore` munging.

## What the wizard asks

1. **Where do your `.prim` source files live?** A directory under the
   project root (e.g. `constants/`, `shared/constants/`). Path
   autocomplete suggests existing directories. If the directory
   doesn't exist yet, `init` tells you so on the spot — you'll
   create it and drop your first `.prim` file in before the next
   `primate build`.
2. **Which languages should primate generate?** Multiselect across
   TypeScript, Rust, and Python. None checked by default — pick the
   ones you actually want.
3. **For each picked language: where should the output go?** Path
   autocomplete on every prompt. No preset value, since the right
   path depends on your repo layout — the help line shows an
   example for shape (`e.g. web/src/generated/constants/`).
4. **Use an external generator plugin?** If yes, loops through name
   / command / path until you say no. See
   [Plugins](../plugins/protocol.md).

The wizard uses [inquire](https://github.com/mikaelmello/inquire) for
prompts. Press <kbd>Esc</kbd> to cancel at any point; nothing is
written until the wizard completes.

## What the result looks like

The generated `primate.toml` is heavily commented and lists every
option each picked generator accepts at its default value. Picking
TypeScript writes the full set of TypeScript options inline:

```toml
[[output]]
generator = "typescript"
# A directory. primate emits one `.ts` per namespace plus
# an `index.ts` that re-exports each one.
path      = "web/src/generated/constants/"

# Generator options. Defaults shown — change as needed.
options.naming      = "camelCase"               # or "SCREAMING_SNAKE_CASE"
options.duration    = "number"                  # or "temporal" for Temporal.Duration
options.u64         = "number"                  # or "bigint" for u64-typed constants
options.enumStyle   = "literal"                 # or "const" or "enum" — see docs
```

Same idea for Rust and Python; only the targets you pick land in the
file (no commented-out scaffolding for skipped ones).

## Flags

- `--force` / `-f` — overwrite an existing `primate.toml`. Default is
  to refuse so an existing config doesn't get clobbered.

## After `primate init`

The wizard prints a small "next steps" block — create the source
directory, drop in a `.prim` file, run `primate build`. There's also
a recommendation to **commit the generated files alongside source**:
consumers don't need primate installed to use them, and CI can gate
on `git diff --exit-code` after `primate build` to catch drift.

## See also

- [`primate build`](./build.md) — the generator pipeline `init`
  configures.
- [`primate fmt`](./fmt.md) — formatter; run after every `.prim` edit.
- [Plugin protocol](../plugins/protocol.md) — what external generators
  need to implement.
