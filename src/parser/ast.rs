//! AST for the primate DSL.
//!
//! The AST preserves enough trivia (comments, blank lines) for the
//! formatter to round-trip files.

use super::lexer::Span;

#[derive(Debug, Clone)]
pub struct File {
    pub items: Vec<Item>,
}

/// Top-level item. Keeps trivia (standalone comments, blank lines) as
/// first-class items so the formatter can preserve their placement.
#[derive(Debug, Clone)]
pub enum Item {
    /// A declaration with its leading doc comment and attributes.
    Decl(Decl),
    /// A `use` import statement (RFC 0003 §3).
    Use(UseDecl),
    /// A standalone `// comment` on its own line (not attached to a decl).
    LineComment { text: String, span: Span },
    /// A `//! file doc` comment line.
    FileDoc { text: String, span: Span },
    /// One or more blank lines between items.
    BlankLine,
}

#[derive(Debug, Clone)]
pub struct UseDecl {
    /// Path segments before the leaf (or before the brace group). For
    /// `use a::b::C` this is `["a", "b"]`; for `use a::b::{C, D}` also `["a", "b"]`.
    pub path: Vec<String>,
    /// Imported names. Single form `use a::b::C` produces `["C"]`; brace form
    /// `use a::b::{C, D}` produces `["C", "D"]`.
    pub items: Vec<UseItem>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct UseItem {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub doc: Option<DocBlock>,
    pub attributes: Vec<Attribute>,
    pub kind: DeclKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct DocBlock {
    /// Each entry is one `///` line, with the leading slash and single space stripped.
    pub lines: Vec<String>,
}

impl DocBlock {
    pub fn joined(&self) -> String {
        self.lines.join("\n")
    }
}

#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<AttrArg>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum AttrArg {
    Ident(String),
    Str(String),
    Int(i128),
    Bool(bool),
}

#[derive(Debug, Clone)]
pub enum DeclKind {
    Namespace(NamespaceDecl),
    Const(ConstDecl),
    Enum(EnumDecl),
    TypeAlias(TypeAliasDecl),
}

#[derive(Debug, Clone)]
pub struct NamespaceDecl {
    pub path: Vec<String>,
    pub path_span: Span,
}

#[derive(Debug, Clone)]
pub struct ConstDecl {
    pub type_expr: TypeExpr,
    pub name: String,
    pub name_span: Span,
    pub value: ValueExpr,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub name_span: Span,
    pub backing: Option<TypeExpr>,
    pub variants: Vec<EnumVariantDecl>,
}

#[derive(Debug, Clone)]
pub struct EnumVariantDecl {
    pub doc: Option<DocBlock>,
    pub name: String,
    pub name_span: Span,
    pub value: Option<ValueExpr>,
}

#[derive(Debug, Clone)]
pub struct TypeAliasDecl {
    pub name: String,
    pub name_span: Span,
    pub target: TypeExpr,
}

#[derive(Debug, Clone)]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeExprKind {
    /// A named type: a primitive (`u32`, `string`), or a user-defined enum/alias.
    /// The path may be qualified (`core::types::LogLevel`) or bare (`LogLevel`).
    Named { path: Vec<String> },
    /// `T[]`
    Array(Box<TypeExpr>),
    /// `T?`
    Optional(Box<TypeExpr>),
    /// `array<T>` (de-sugars to Array; kept distinct for round-tripping if desired)
    ArrayGeneric(Box<TypeExpr>),
    /// `array<T, N>` — fixed-size homogeneous array (RFC 0003 §2).
    FixedArrayGeneric { element: Box<TypeExpr>, length: u32 },
    /// `optional<T>`
    OptionalGeneric(Box<TypeExpr>),
    /// `map<K, V>`
    Map {
        key: Box<TypeExpr>,
        value: Box<TypeExpr>,
    },
    /// `tuple<A, B, ...>`
    Tuple(Vec<TypeExpr>),
}

#[derive(Debug, Clone)]
pub struct ValueExpr {
    pub kind: ValueExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ValueExprKind {
    Int {
        value: i128,
        suffix: Option<String>,
    },
    Float {
        value: f64,
        suffix: Option<String>,
    },
    Bool(bool),
    Str(String),
    /// `none` literal — the optional-empty case.
    None_,
    /// A bare or qualified identifier path used as a value (enum variant).
    /// e.g. `Info`, `core::types::LogLevel::Info`.
    Path {
        path: Vec<String>,
    },
    /// `[...]` ordered literal. Lower-pass disambiguates against the declared
    /// type to either an array or tuple. `trailing_comma` records whether the
    /// source had a comma after the last element — RFC 0003 §4 magic-trailing-
    /// comma signal that keeps the formatter in multi-line mode.
    Array {
        items: Vec<ValueExpr>,
        trailing_comma: bool,
    },
    /// `{ key: value, ... }` map literal.
    Map {
        entries: Vec<(MapKey, ValueExpr)>,
        trailing_comma: bool,
    },
    /// Tuple literal. The parser no longer produces this (RFC 0003 §1 — tuple
    /// values use `[...]`). Kept for hand-built ASTs.
    Tuple {
        items: Vec<ValueExpr>,
        trailing_comma: bool,
    },
    /// Negation: `-5`, `-30s`. Operand is always a numeric literal.
    Neg(Box<ValueExpr>),
}

#[derive(Debug, Clone)]
pub struct MapKey {
    pub kind: MapKeyKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum MapKeyKind {
    Str(String),
    Ident(String),
    Int(i128),
}
