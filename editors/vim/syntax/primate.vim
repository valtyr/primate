" Vim syntax file for primate (`.prim`)
"
" Drop into ~/.vim/syntax/ (or use the ftdetect file alongside it).
" Designed for the primate DSL described in rfc/0002-primate-syntax.md.

if exists("b:current_syntax")
  finish
endif

" Comments
syn match primateComment       "//[^!/].*$"  contains=@Spell
syn match primateDocComment    "///.*$"      contains=@Spell
syn match primateFileDoc       "//!.*$"      contains=@Spell

" Keywords
syn keyword primateKeyword     namespace enum type
syn keyword primateBoolean     true false
syn keyword primateNone        none

" Primitive types
syn keyword primatePrimType    i8 i16 i32 i64 u8 u16 u32 u64 f32 f64
syn keyword primatePrimType    bool string duration bytes regex url
syn keyword primateContainer   array optional map tuple

" Constant names (SCREAMING_SNAKE_CASE)
syn match   primateConstName   "\<[A-Z][A-Z0-9_]*\>"

" PascalCase identifiers — enums, aliases, variants
syn match   primateTypeName    "\<[A-Z][a-zA-Z0-9]*\>"

" Numbers
syn match   primateNumberHex   "\<0x[0-9A-Fa-f_]\+\>"
syn match   primateNumberBin   "\<0b[01_]\+\>"
syn match   primateNumberOct   "\<0o[0-7_]\+\>"
syn match   primateNumber      "\<\d[0-9_]*\(\.\d[0-9_]*\)\?\([eE][+-]\?\d\+\)\?\([A-Za-z]\+\)\?\>"

" Strings
syn region  primateString      start=+"+ skip=+\\.+ end=+"+ contains=primateEscape
syn region  primateRawString   start=+r#*"+ end=+"#*+
syn match   primateEscape      contained "\\\([nrt0\\\"]\)"

" Attributes
syn match   primateAttribute   "@[A-Za-z_][A-Za-z0-9_]*"

" Punctuation (subtle)
syn match   primateDelim       "::\|->\|[<>?\[\](){},:=]"

hi def link primateComment     Comment
hi def link primateDocComment  SpecialComment
hi def link primateFileDoc     SpecialComment
hi def link primateKeyword     Keyword
hi def link primateBoolean     Boolean
hi def link primateNone        Constant
hi def link primatePrimType    Type
hi def link primateContainer   Type
hi def link primateConstName   Identifier
hi def link primateTypeName    Type
hi def link primateNumber      Number
hi def link primateNumberHex   Number
hi def link primateNumberBin   Number
hi def link primateNumberOct   Number
hi def link primateString      String
hi def link primateRawString   String
hi def link primateEscape      SpecialChar
hi def link primateAttribute   PreProc
hi def link primateDelim       Delimiter

let b:current_syntax = "primate"
