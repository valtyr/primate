# Vim

primate ships syntax and ftdetect files for Vim/Neovim at
`editors/vim/`. They handle highlighting; LSP features are wired
through your favorite client (`coc.nvim`, `nvim-lspconfig`, `vim-lsp`).

## Syntax + ftdetect

Drop the files into your runtime path:

```bash
cp editors/vim/syntax/primate.vim    ~/.vim/syntax/
cp editors/vim/ftdetect/primate.vim  ~/.vim/ftdetect/
```

Or for Neovim:

```bash
cp editors/vim/syntax/primate.vim    ~/.config/nvim/syntax/
cp editors/vim/ftdetect/primate.vim  ~/.config/nvim/ftdetect/
```

After this, `*.prim` files auto-detect as primate and pick up syntax
highlighting (keywords, strings, numbers, doc comments, attributes).

## LSP

Wire `primate lsp` into your LSP client. Examples below.

### nvim-lspconfig

```lua
local lspconfig = require('lspconfig')
local configs   = require('lspconfig.configs')

if not configs.primate then
  configs.primate = {
    default_config = {
      cmd       = { 'primate', 'lsp' },
      filetypes = { 'primate' },
      root_dir  = lspconfig.util.root_pattern('primate.toml', '.git'),
      settings  = {},
    },
  }
end

lspconfig.primate.setup({})
```

### coc.nvim

In `coc-settings.json`:

```jsonc
{
  "languageserver": {
    "primate": {
      "command":     "primate",
      "args":        ["lsp"],
      "filetypes":   ["primate"],
      "rootPatterns": ["primate.toml", ".git"]
    }
  }
}
```

### vim-lsp

```vim
if executable('primate')
  au User lsp_setup call lsp#register_server({
    \ 'name': 'primate',
    \ 'cmd': {server_info -> ['primate', 'lsp']},
    \ 'allowlist': ['primate'],
    \ })
endif
```

## Verifying it works

Open one of `examples/constants/*.prim`. You should see:

- Keywords (`enum`, `type`, `namespace`, `use`) highlighted as
  `Keyword`.
- Doc comments (`///`) styled distinctly from regular line comments.
- Numeric literals with their unit suffixes — `30s`, `100MiB` —
  visually distinct from plain numbers.

If LSP is wired up, `:LspHover` (or your client's equivalent) on a type
name shows its kind, namespace, and doc comment. `:LspDefinition` jumps
to the declaration.

## Format on save

primate has no native autocmd, but you can wire `primate fmt` into a
buffer-local `BufWritePre` or use your LSP client's `formatExpr`. With
nvim-lspconfig:

```lua
vim.api.nvim_create_autocmd('BufWritePre', {
  pattern = '*.prim',
  callback = function() vim.lsp.buf.format() end,
})
```
