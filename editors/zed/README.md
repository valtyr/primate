# primate — Zed extension

Zed language support for [`primate`](https://github.com/valtyr/primate),
a small DSL and code generator for cross-language constants. Write your
shared values once; generate typed Rust, TypeScript, and Python.

## Features

- Tree-sitter syntax highlighting for `.prim` files.
- LSP integration (talks to the `primate lsp` binary that ships with the
  CLI): diagnostics, hover docs, go-to-definition, find-references,
  format-on-save, contextual completion (enum variants, unit suffixes).

## Setup

1. Install the [primate CLI](https://github.com/valtyr/primate):

   ```sh
   cargo install primate --locked
   ```

   The extension shells out to `primate lsp`. The CLI must be on `PATH`.
   To point Zed at a custom binary, add this to your `settings.json`:

   ```jsonc
   {
     "lsp": {
       "primate": {
         "binary": {
           "path": "/absolute/path/to/primate"
         }
       }
     }
   }
   ```

2. Install this extension from the Zed extensions registry
   (**zed: extensions** → search "primate" → Install).

3. Open any `.prim` file. You should see syntax highlighting and
   diagnostics inline as you type.

## See also

- [Project README](https://github.com/valtyr/primate#readme)
- [Documentation](https://valtyr.github.io/primate/)
