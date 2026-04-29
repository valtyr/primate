# Grammar

A reference grammar for `.prim` files. EBNF-flavored notation; the
canonical implementation lives in `src/parser/grammar.rs`.

```ebnf
File          ::= ( NamespaceLine NEWLINE )?
                  ( UseStatement NEWLINE )*
                  ( Item NEWLINE? )*

Item          ::= Decl
                | LineComment
                | DocComment
                | FileDocComment
                | BlankLine

NamespaceLine ::= "namespace" Path

Path          ::= Ident ( "::" Ident )*

UseStatement  ::= "use" Path                              // single
                | "use" Path "::" "{" UseList "}"          // brace

UseList       ::= Ident ( "," Ident )* ","?

Decl          ::= ( DocBlock )? ( Attribute NEWLINE )*
                  ( ConstDecl | EnumDecl | TypeAliasDecl )

ConstDecl     ::= TypeExpr Ident "=" Value
                  // Ident is SCREAMING_SNAKE_CASE

EnumDecl      ::= "enum" Ident ( ":" TypeExpr )? "{"
                  ( EnumVariant ( "," EnumVariant )* ","? )?
                  "}"

EnumVariant   ::= ( DocBlock )? Ident ( "=" Value )?

TypeAliasDecl ::= "type" Ident "=" TypeExpr

TypeExpr      ::= Path                                     // bare or qualified
                | TypeExpr "[]"                            // array sugar
                | TypeExpr "?"                             // optional sugar
                | "array"    "<" TypeExpr ">"
                | "array"    "<" TypeExpr "," IntLit ">"   // fixed-size
                | "optional" "<" TypeExpr ">"
                | "map"      "<" TypeExpr "," TypeExpr ">"
                | "tuple"    "<" TypeExpr ( "," TypeExpr )* ","? ">"

Value         ::= IntLit | FloatLit | StrLit | BoolLit
                | "none"
                | Path                                     // enum variant
                | "[" ValueList? "]"                       // array / tuple
                | "{" MapEntries? "}"                      // map
                | "-" ( IntLit | FloatLit )                // negation

ValueList     ::= Value ( "," Value )* ","?

MapEntries    ::= MapEntry ( "," MapEntry )* ","?
MapEntry      ::= MapKey ":" Value
MapKey        ::= StrLit | Ident | IntLit

Attribute     ::= "@" Ident ( "(" AttrArgs? ")" )?
AttrArgs      ::= AttrArg ( "," AttrArg )* ","?
AttrArg       ::= Ident | StrLit | IntLit | BoolLit

DocBlock      ::= ( "///" RestOfLine NEWLINE )+
LineComment   ::= "//" RestOfLine
DocComment    ::= "///" RestOfLine
FileDocComment::= "//!" RestOfLine

Ident         ::= [A-Za-z_][A-Za-z0-9_]*

IntLit        ::= ( "0x" HexDigit+ | "0b" BinDigit+ | "0o" OctDigit+
                  | DecDigit+ ) UnitSuffix?
FloatLit      ::= DecDigit+ "." DecDigit+ ( [eE] [+-]? DecDigit+ )? UnitSuffix?

StrLit        ::= '"' StringChar* '"'
                | "r" "#"* '"' RawStringChar* '"' "#"*

BoolLit       ::= "true" | "false"

UnitSuffix    ::= [A-Za-z]+    // e.g. ms, s, h, KiB, MiB, GiB
```

## Rules of thumb

- **Newlines terminate top-level items**, with one exception: inside
  `<>`, `[]`, `{}`, `()`, newlines are insignificant. This is what
  lets long type expressions and multi-line value literals work.
- **One declaration per line** outside collection delimiters.
- **No semicolons.** Newlines do the same job and there's no
  expression context where ASI-style ambiguity could arise.
- **Trailing commas** are accepted in every comma-separated list. On
  value-side `[…]` and `{…}` literals, a trailing comma signals
  "keep me multi-line" to the formatter — see
  [Values](../language/values.md).

## Reserved tokens

Keywords (lex-time): `namespace`, `enum`, `type`, `use`, `as`, `true`,
`false`, `none`.

`as` is reserved for the future `use a::b::C as D` form (RFC 0003).

## Lexical structure

Whitespace is space, tab, carriage return, and newline. Newlines are
significant tokens (the lexer emits `Newline` and, for blank-line
runs, `BlankLine`). Inside delimiters the parser consumes them as
trivia.

Comments are lexed as `// ...`, `/// ...`, or `//! ...` to end of
line. Block comments (`/* */`) are explicitly rejected with a
diagnostic.

Underscores are accepted in numeric literals as digit separators
(`1_000_000`, `0xFF_FF`). They have no semantic value.

## Differences from RFC text

This grammar is the implementation reference. Where it disagrees with
[RFC 0002](https://github.com/valtyr/primate/blob/main/rfc/0002-primate-syntax.md)
or [RFC 0003](https://github.com/valtyr/primate/blob/main/rfc/0003-tuples-arrays-use-wrapping.md),
the implementation wins; the RFCs are decision records, not specs.
