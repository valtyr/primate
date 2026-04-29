//! Recursive-descent grammar for the primate DSL.
//!
//! Consumes the token stream produced by `lexer.rs` and produces an AST
//! that preserves enough trivia for the formatter to round-trip files.

use super::ast::*;
use super::lexer::{Span, Tok, Token};

#[derive(Debug, Clone)]
pub struct ParseDiag {
    pub message: String,
    pub span: Span,
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    pub diags: Vec<ParseDiag>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            diags: Vec::new(),
        }
    }

    pub fn parse_file(&mut self) -> File {
        let mut items = Vec::new();
        loop {
            // Pending trivia: collect doc comments (attached to next decl)
            // and standalone comments (top-level items).
            let mut pending_docs: Vec<(String, Span)> = Vec::new();
            let mut pending_attrs: Vec<Attribute> = Vec::new();
            // Track if we just emitted a blank line so we can decide whether
            // to keep pending docs or flush them as standalone comments.
            loop {
                match self.peek_kind() {
                    Tok::Eof => break,
                    Tok::FileDocComment(text) => {
                        let span = self.peek_span();
                        let text = text.clone();
                        self.bump();
                        // Preserve a blank line after the file doc, if present.
                        let was_blank = matches!(self.peek_kind(), Tok::BlankLine);
                        if matches!(self.peek_kind(), Tok::Newline | Tok::BlankLine) {
                            self.bump();
                        }
                        items.push(Item::FileDoc { text, span });
                        if was_blank {
                            items.push(Item::BlankLine);
                        }
                    }
                    Tok::DocComment(text) => {
                        let span = self.peek_span();
                        let t = text.clone();
                        self.bump();
                        if matches!(self.peek_kind(), Tok::Newline) {
                            self.bump();
                            pending_docs.push((t, span));
                        } else if matches!(self.peek_kind(), Tok::BlankLine) {
                            self.bump();
                            // Blank line after a doc detaches it. Flush as line comments.
                            for (text, span) in pending_docs.drain(..) {
                                items.push(Item::LineComment {
                                    text: format!("/ {}", text),
                                    span,
                                });
                            }
                            items.push(Item::LineComment {
                                text: format!("/ {}", t),
                                span,
                            });
                            items.push(Item::BlankLine);
                        } else {
                            pending_docs.push((t, span));
                        }
                    }
                    Tok::LineComment(text) => {
                        let span = self.peek_span();
                        let text = text.clone();
                        self.bump();
                        // Eat optional trailing newline
                        if matches!(self.peek_kind(), Tok::Newline) {
                            self.bump();
                        }
                        // If we had pending docs, flush them as standalone comments
                        // because a non-doc comment broke the doc block.
                        for (dtext, dspan) in pending_docs.drain(..) {
                            items.push(Item::LineComment {
                                text: format!("/ {}", dtext),
                                span: dspan,
                            });
                        }
                        // Drop pending attributes too (they would have attached to
                        // the doc); emit a diagnostic if any.
                        if !pending_attrs.is_empty() {
                            for attr in pending_attrs.drain(..) {
                                self.diags.push(ParseDiag {
                                    message: "attribute is not attached to a declaration"
                                        .to_string(),
                                    span: attr.span,
                                });
                            }
                        }
                        items.push(Item::LineComment { text, span });
                    }
                    Tok::Newline => {
                        self.bump();
                    }
                    Tok::BlankLine => {
                        self.bump();
                        // Blank line breaks a doc block too.
                        for (dtext, dspan) in pending_docs.drain(..) {
                            items.push(Item::LineComment {
                                text: format!("/ {}", dtext),
                                span: dspan,
                            });
                        }
                        if !pending_attrs.is_empty() {
                            for attr in pending_attrs.drain(..) {
                                self.diags.push(ParseDiag {
                                    message: "attribute is not attached to a declaration"
                                        .to_string(),
                                    span: attr.span,
                                });
                            }
                        }
                        items.push(Item::BlankLine);
                    }
                    Tok::At => match self.parse_attribute() {
                        Ok(attr) => {
                            pending_attrs.push(attr);
                            if matches!(self.peek_kind(), Tok::Newline) {
                                self.bump();
                            }
                        }
                        Err(()) => {
                            self.recover_to_newline();
                        }
                    },
                    _ => break,
                }
            }

            if matches!(self.peek_kind(), Tok::Eof) {
                break;
            }

            // `use` is its own top-level item — no docs / no attributes.
            if matches!(self.peek_kind(), Tok::KwUse) {
                // Doc comments and attributes don't attach to `use`. If there
                // were any pending, flush docs as standalone line comments and
                // diagnose attributes.
                for (text, span) in pending_docs.drain(..) {
                    items.push(Item::LineComment {
                        text: format!("/ {}", text),
                        span,
                    });
                }
                for attr in pending_attrs.drain(..) {
                    self.diags.push(ParseDiag {
                        message: "attribute does not attach to a `use` statement".to_string(),
                        span: attr.span,
                    });
                }
                match self.parse_use() {
                    Ok(u) => items.push(Item::Use(u)),
                    Err(()) => self.recover_to_newline(),
                }
                if matches!(self.peek_kind(), Tok::Newline | Tok::BlankLine) {
                    if matches!(self.peek_kind(), Tok::BlankLine) {
                        self.bump();
                        items.push(Item::BlankLine);
                    } else {
                        self.bump();
                    }
                }
                continue;
            }

            // Now we expect a declaration.
            let decl_start = self.peek_span();
            let doc = if pending_docs.is_empty() {
                None
            } else {
                Some(DocBlock {
                    lines: pending_docs.into_iter().map(|(t, _)| t).collect(),
                })
            };
            let attributes = pending_attrs;

            match self.parse_decl_kind() {
                Ok(kind) => {
                    let end = self.last_span();
                    let span = Span {
                        start: decl_start.start,
                        end: end.end,
                        line: decl_start.line,
                        column: decl_start.column,
                    };
                    items.push(Item::Decl(Decl {
                        doc,
                        attributes,
                        kind,
                        span,
                    }));
                }
                Err(()) => {
                    self.recover_to_newline();
                }
            }

            // Consume the trailing newline if present
            if matches!(self.peek_kind(), Tok::Newline | Tok::BlankLine) {
                if matches!(self.peek_kind(), Tok::BlankLine) {
                    self.bump();
                    items.push(Item::BlankLine);
                } else {
                    self.bump();
                }
            }
        }

        File { items }
    }

    fn parse_attribute(&mut self) -> Result<Attribute, ()> {
        let start = self.peek_span();
        self.expect(Tok::At)?;
        let (name, _) = self.expect_ident()?;
        let mut args = Vec::new();
        if matches!(self.peek_kind(), Tok::LParen) {
            self.bump();
            loop {
                self.skip_newlines();
                if matches!(self.peek_kind(), Tok::RParen) {
                    self.bump();
                    break;
                }
                let arg = self.parse_attr_arg()?;
                args.push(arg);
                self.skip_newlines();
                match self.peek_kind() {
                    Tok::Comma => {
                        self.bump();
                    }
                    Tok::RParen => {
                        self.bump();
                        break;
                    }
                    _ => {
                        let span = self.peek_span();
                        self.diags.push(ParseDiag {
                            message: "expected ',' or ')' in attribute arguments".to_string(),
                            span,
                        });
                        return Err(());
                    }
                }
            }
        }
        let end = self.last_span();
        Ok(Attribute {
            name,
            args,
            span: Span {
                start: start.start,
                end: end.end,
                line: start.line,
                column: start.column,
            },
        })
    }

    fn parse_attr_arg(&mut self) -> Result<AttrArg, ()> {
        let span = self.peek_span();
        match self.peek_kind().clone() {
            Tok::Ident(s) => {
                self.bump();
                Ok(AttrArg::Ident(s))
            }
            Tok::Str(s) => {
                self.bump();
                Ok(AttrArg::Str(s))
            }
            Tok::Int { value, .. } => {
                self.bump();
                Ok(AttrArg::Int(value))
            }
            Tok::KwTrue => {
                self.bump();
                Ok(AttrArg::Bool(true))
            }
            Tok::KwFalse => {
                self.bump();
                Ok(AttrArg::Bool(false))
            }
            _ => {
                self.diags.push(ParseDiag {
                    message: "attribute argument must be an identifier, string, integer, or bool"
                        .to_string(),
                    span,
                });
                Err(())
            }
        }
    }

    fn parse_decl_kind(&mut self) -> Result<DeclKind, ()> {
        match self.peek_kind() {
            Tok::KwNamespace => self.parse_namespace().map(DeclKind::Namespace),
            Tok::KwEnum => self.parse_enum().map(DeclKind::Enum),
            Tok::KwType => self.parse_type_alias().map(DeclKind::TypeAlias),
            _ => self.parse_const().map(DeclKind::Const),
        }
    }

    fn parse_use(&mut self) -> Result<UseDecl, ()> {
        let start = self.peek_span();
        self.expect(Tok::KwUse)?;

        // Parse the leading path: at least one identifier, then `::Ident` segments.
        // The parse stops when the next token is either `{` (brace form) or
        // when we've just consumed an identifier and the following token is
        // *not* `::` (single form — the last identifier becomes the imported
        // name).
        let mut path: Vec<String> = Vec::new();
        let mut last_ident: Option<(String, Span)> = None;
        loop {
            if path.is_empty() && last_ident.is_none() {
                let (name, span) = self.expect_ident()?;
                last_ident = Some((name, span));
            }

            if matches!(self.peek_kind(), Tok::ColonColon) {
                // Push last ident into path and look at what follows the `::`.
                if let Some((name, _)) = last_ident.take() {
                    path.push(name);
                }
                self.bump();
                if matches!(self.peek_kind(), Tok::LBrace) {
                    break;
                }
                let (name, span) = self.expect_ident()?;
                last_ident = Some((name, span));
                continue;
            }
            // No more `::` — last_ident is the leaf in the single form.
            break;
        }

        let mut items: Vec<UseItem> = Vec::new();
        if matches!(self.peek_kind(), Tok::LBrace) {
            // Brace form: `use a::b::{X, Y}`.
            self.bump();
            loop {
                self.skip_newlines();
                if matches!(self.peek_kind(), Tok::RBrace) {
                    self.bump();
                    break;
                }
                let (name, span) = self.expect_ident()?;
                items.push(UseItem { name, span });
                self.skip_newlines();
                match self.peek_kind() {
                    Tok::Comma => {
                        self.bump();
                    }
                    Tok::RBrace => {
                        self.bump();
                        break;
                    }
                    _ => {
                        let span = self.peek_span();
                        self.diags.push(ParseDiag {
                            message: "expected ',' or '}' in use list".to_string(),
                            span,
                        });
                        return Err(());
                    }
                }
            }
            if items.is_empty() {
                self.diags.push(ParseDiag {
                    message: "empty `use { }` group".to_string(),
                    span: start,
                });
                return Err(());
            }
        } else {
            // Single form: the last identifier is the imported leaf.
            match last_ident {
                Some((name, span)) => items.push(UseItem { name, span }),
                None => {
                    self.diags.push(ParseDiag {
                        message: "expected an identifier or `{ ... }` after `use`".to_string(),
                        span: start,
                    });
                    return Err(());
                }
            }
        }

        if path.is_empty() {
            self.diags.push(ParseDiag {
                message: "`use` must reference at least one namespace segment, e.g. `use a::B`"
                    .to_string(),
                span: start,
            });
            return Err(());
        }

        let end = self.last_span();
        Ok(UseDecl {
            path,
            items,
            span: Span {
                start: start.start,
                end: end.end,
                line: start.line,
                column: start.column,
            },
        })
    }

    fn parse_namespace(&mut self) -> Result<NamespaceDecl, ()> {
        self.expect(Tok::KwNamespace)?;
        let start = self.peek_span();
        let (head, _) = self.expect_ident()?;
        let mut path = vec![head];
        let mut last = self.last_span();
        while matches!(self.peek_kind(), Tok::ColonColon) {
            self.bump();
            let (seg, sp) = self.expect_ident()?;
            path.push(seg);
            last = sp;
        }
        Ok(NamespaceDecl {
            path,
            path_span: Span {
                start: start.start,
                end: last.end,
                line: start.line,
                column: start.column,
            },
        })
    }

    fn parse_enum(&mut self) -> Result<EnumDecl, ()> {
        self.expect(Tok::KwEnum)?;
        let (name, name_span) = self.expect_ident()?;
        let backing = if matches!(self.peek_kind(), Tok::Colon) {
            self.bump();
            Some(self.parse_type_expr()?)
        } else {
            None
        };
        self.expect(Tok::LBrace)?;
        // Allow newlines/comments inside the body.
        let mut variants = Vec::new();
        loop {
            self.skip_newlines();
            // Collect any /// docs
            let mut pending_docs: Vec<String> = Vec::new();
            while let Tok::DocComment(text) = self.peek_kind() {
                let t = text.clone();
                self.bump();
                pending_docs.push(t);
                if matches!(self.peek_kind(), Tok::Newline) {
                    self.bump();
                }
            }
            self.skip_newlines();

            if matches!(self.peek_kind(), Tok::RBrace) {
                self.bump();
                break;
            }

            let (vname, vspan) = self.expect_ident()?;
            let value = if matches!(self.peek_kind(), Tok::Eq) {
                self.bump();
                Some(self.parse_value_expr()?)
            } else {
                None
            };
            variants.push(EnumVariantDecl {
                doc: if pending_docs.is_empty() {
                    None
                } else {
                    Some(DocBlock {
                        lines: pending_docs,
                    })
                },
                name: vname,
                name_span: vspan,
                value,
            });
            // Optional trailing comma; allow newline as separator too.
            if matches!(self.peek_kind(), Tok::Comma) {
                self.bump();
            }
        }

        Ok(EnumDecl {
            name,
            name_span,
            backing,
            variants,
        })
    }

    fn parse_type_alias(&mut self) -> Result<TypeAliasDecl, ()> {
        self.expect(Tok::KwType)?;
        let (name, name_span) = self.expect_ident()?;
        self.expect(Tok::Eq)?;
        let target = self.parse_type_expr()?;
        Ok(TypeAliasDecl {
            name,
            name_span,
            target,
        })
    }

    fn parse_const(&mut self) -> Result<ConstDecl, ()> {
        let type_expr = self.parse_type_expr()?;
        let (name, name_span) = self.expect_ident()?;
        self.expect(Tok::Eq)?;
        let value = self.parse_value_expr()?;
        Ok(ConstDecl {
            type_expr,
            name,
            name_span,
            value,
        })
    }

    fn parse_type_expr(&mut self) -> Result<TypeExpr, ()> {
        let start = self.peek_span();
        let mut kind = match self.peek_kind().clone() {
            Tok::Ident(s) => {
                self.bump();
                let mut path = vec![s];
                while matches!(self.peek_kind(), Tok::ColonColon) {
                    self.bump();
                    let (seg, _) = self.expect_ident()?;
                    path.push(seg);
                }
                let head = &path[0];
                if path.len() == 1 {
                    match head.as_str() {
                        "array" => {
                            self.expect(Tok::Lt)?;
                            self.skip_newlines();
                            let inner = self.parse_type_expr()?;
                            self.skip_newlines();
                            // Optional fixed length: `array<T, N>` (RFC 0003 §2).
                            let kind = if matches!(self.peek_kind(), Tok::Comma) {
                                self.bump();
                                self.skip_newlines();
                                let len_span = self.peek_span();
                                let length = match self.peek_kind().clone() {
                                    Tok::Int { value, suffix } => {
                                        self.bump();
                                        if suffix.is_some() {
                                            self.diags.push(ParseDiag {
                                                message: "array length cannot have a unit suffix"
                                                    .to_string(),
                                                span: len_span,
                                            });
                                            return Err(());
                                        }
                                        if value < 0 {
                                            self.diags.push(ParseDiag {
                                                message: "array length must be non-negative"
                                                    .to_string(),
                                                span: len_span,
                                            });
                                            return Err(());
                                        }
                                        if value > u32::MAX as i128 {
                                            self.diags.push(ParseDiag {
                                                message: "array length exceeds u32::MAX"
                                                    .to_string(),
                                                span: len_span,
                                            });
                                            return Err(());
                                        }
                                        value as u32
                                    }
                                    other => {
                                        self.diags.push(ParseDiag {
                                            message: format!(
                                                "expected integer length, got {}",
                                                describe_tok(&other)
                                            ),
                                            span: len_span,
                                        });
                                        return Err(());
                                    }
                                };
                                self.skip_newlines();
                                if matches!(self.peek_kind(), Tok::Comma) {
                                    self.bump();
                                    self.skip_newlines();
                                }
                                TypeExprKind::FixedArrayGeneric {
                                    element: Box::new(inner),
                                    length,
                                }
                            } else {
                                TypeExprKind::ArrayGeneric(Box::new(inner))
                            };
                            self.expect(Tok::Gt)?;
                            kind
                        }
                        "optional" => {
                            self.expect(Tok::Lt)?;
                            self.skip_newlines();
                            let inner = self.parse_type_expr()?;
                            self.skip_newlines();
                            self.expect(Tok::Gt)?;
                            TypeExprKind::OptionalGeneric(Box::new(inner))
                        }
                        "map" => {
                            self.expect(Tok::Lt)?;
                            self.skip_newlines();
                            let key = self.parse_type_expr()?;
                            self.skip_newlines();
                            self.expect(Tok::Comma)?;
                            self.skip_newlines();
                            let value = self.parse_type_expr()?;
                            self.skip_newlines();
                            // Optional trailing comma.
                            if matches!(self.peek_kind(), Tok::Comma) {
                                self.bump();
                                self.skip_newlines();
                            }
                            self.expect(Tok::Gt)?;
                            TypeExprKind::Map {
                                key: Box::new(key),
                                value: Box::new(value),
                            }
                        }
                        "tuple" => {
                            self.expect(Tok::Lt)?;
                            let mut elems = Vec::new();
                            loop {
                                self.skip_newlines();
                                if matches!(self.peek_kind(), Tok::Gt) {
                                    self.bump();
                                    break;
                                }
                                elems.push(self.parse_type_expr()?);
                                self.skip_newlines();
                                match self.peek_kind() {
                                    Tok::Comma => {
                                        self.bump();
                                    }
                                    Tok::Gt => {
                                        self.bump();
                                        break;
                                    }
                                    _ => {
                                        let span = self.peek_span();
                                        self.diags.push(ParseDiag {
                                            message: "expected ',' or '>' in tuple type"
                                                .to_string(),
                                            span,
                                        });
                                        return Err(());
                                    }
                                }
                            }
                            TypeExprKind::Tuple(elems)
                        }
                        _ => TypeExprKind::Named { path },
                    }
                } else {
                    TypeExprKind::Named { path }
                }
            }
            other => {
                self.diags.push(ParseDiag {
                    message: format!("expected a type, got {}", describe_tok(&other)),
                    span: start,
                });
                return Err(());
            }
        };

        // Sugar suffixes: T[] and T?
        loop {
            match self.peek_kind() {
                Tok::LBracket => {
                    self.bump();
                    if !matches!(self.peek_kind(), Tok::RBracket) {
                        let span = self.peek_span();
                        self.diags.push(ParseDiag {
                            message: "expected ']' to close array suffix".to_string(),
                            span,
                        });
                        return Err(());
                    }
                    self.bump();
                    let end = self.last_span();
                    kind = TypeExprKind::Array(Box::new(TypeExpr {
                        kind,
                        span: Span {
                            start: start.start,
                            end: end.end,
                            line: start.line,
                            column: start.column,
                        },
                    }));
                }
                Tok::Question => {
                    self.bump();
                    let end = self.last_span();
                    kind = TypeExprKind::Optional(Box::new(TypeExpr {
                        kind,
                        span: Span {
                            start: start.start,
                            end: end.end,
                            line: start.line,
                            column: start.column,
                        },
                    }));
                }
                _ => break,
            }
        }

        let end = self.last_span();
        Ok(TypeExpr {
            kind,
            span: Span {
                start: start.start,
                end: end.end,
                line: start.line,
                column: start.column,
            },
        })
    }

    fn parse_value_expr(&mut self) -> Result<ValueExpr, ()> {
        let span = self.peek_span();
        let kind = match self.peek_kind().clone() {
            Tok::Int { value, suffix } => {
                self.bump();
                ValueExprKind::Int { value, suffix }
            }
            Tok::Float { value, suffix } => {
                self.bump();
                ValueExprKind::Float { value, suffix }
            }
            Tok::Str(s) => {
                self.bump();
                ValueExprKind::Str(s)
            }
            Tok::KwTrue => {
                self.bump();
                ValueExprKind::Bool(true)
            }
            Tok::KwFalse => {
                self.bump();
                ValueExprKind::Bool(false)
            }
            Tok::KwNone => {
                self.bump();
                ValueExprKind::None_
            }
            Tok::Minus => {
                self.bump();
                let inner = self.parse_value_expr()?;
                if !matches!(
                    inner.kind,
                    ValueExprKind::Int { .. } | ValueExprKind::Float { .. }
                ) {
                    self.diags.push(ParseDiag {
                        message: "unary minus only applies to numeric literals".to_string(),
                        span,
                    });
                }
                ValueExprKind::Neg(Box::new(inner))
            }
            Tok::LBracket => {
                self.bump();
                let mut elems = Vec::new();
                let mut trailing_comma = false;
                loop {
                    self.skip_newlines();
                    if matches!(self.peek_kind(), Tok::RBracket) {
                        self.bump();
                        break;
                    }
                    elems.push(self.parse_value_expr()?);
                    self.skip_newlines();
                    match self.peek_kind() {
                        Tok::Comma => {
                            self.bump();
                            self.skip_newlines();
                            // RFC 0003 §4 magic trailing comma: a comma followed
                            // immediately by `]` (modulo newlines) means the
                            // user opted into multi-line formatting.
                            if matches!(self.peek_kind(), Tok::RBracket) {
                                trailing_comma = true;
                                self.bump();
                                break;
                            }
                        }
                        Tok::RBracket => {
                            self.bump();
                            break;
                        }
                        _ => {
                            let span = self.peek_span();
                            self.diags.push(ParseDiag {
                                message: "expected ',' or ']' in array literal".to_string(),
                                span,
                            });
                            return Err(());
                        }
                    }
                }
                ValueExprKind::Array {
                    items: elems,
                    trailing_comma,
                }
            }
            Tok::LBrace => {
                self.bump();
                let mut entries: Vec<(MapKey, ValueExpr)> = Vec::new();
                let mut trailing_comma = false;
                loop {
                    self.skip_newlines();
                    if matches!(self.peek_kind(), Tok::RBrace) {
                        self.bump();
                        break;
                    }
                    let key_span = self.peek_span();
                    let key_kind = match self.peek_kind().clone() {
                        Tok::Str(s) => {
                            self.bump();
                            MapKeyKind::Str(s)
                        }
                        Tok::Ident(s) => {
                            self.bump();
                            MapKeyKind::Ident(s)
                        }
                        Tok::Int { value, .. } => {
                            self.bump();
                            MapKeyKind::Int(value)
                        }
                        other => {
                            self.diags.push(ParseDiag {
                                message: format!(
                                    "expected map key (string, identifier, or integer), got {}",
                                    describe_tok(&other)
                                ),
                                span: key_span,
                            });
                            return Err(());
                        }
                    };
                    self.skip_newlines();
                    self.expect(Tok::Colon)?;
                    self.skip_newlines();
                    let value = self.parse_value_expr()?;
                    entries.push((
                        MapKey {
                            kind: key_kind,
                            span: key_span,
                        },
                        value,
                    ));
                    self.skip_newlines();
                    match self.peek_kind() {
                        Tok::Comma => {
                            self.bump();
                            self.skip_newlines();
                            if matches!(self.peek_kind(), Tok::RBrace) {
                                trailing_comma = true;
                                self.bump();
                                break;
                            }
                        }
                        Tok::RBrace => {
                            self.bump();
                            break;
                        }
                        _ => {
                            let span = self.peek_span();
                            self.diags.push(ParseDiag {
                                message: "expected ',' or '}' in map literal".to_string(),
                                span,
                            });
                            return Err(());
                        }
                    }
                }
                ValueExprKind::Map {
                    entries,
                    trailing_comma,
                }
            }
            Tok::LParen => {
                // RFC 0003 §1: tuple values use `[...]`, not `(...)`. The
                // `(...)` form was the original syntax pre-RFC-0003 and is
                // rejected here with a clear migration message.
                self.diags.push(ParseDiag {
                    message: "tuple values use `[...]`, not `(...)`".to_string(),
                    span,
                });
                return Err(());
            }
            Tok::Ident(s) => {
                self.bump();
                let mut path = vec![s];
                while matches!(self.peek_kind(), Tok::ColonColon) {
                    self.bump();
                    let (seg, _) = self.expect_ident()?;
                    path.push(seg);
                }
                ValueExprKind::Path { path }
            }
            other => {
                self.diags.push(ParseDiag {
                    message: format!("expected a value, got {}", describe_tok(&other)),
                    span,
                });
                return Err(());
            }
        };
        let end = self.last_span();
        Ok(ValueExpr {
            kind,
            span: Span {
                start: span.start,
                end: end.end,
                line: span.line,
                column: span.column,
            },
        })
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek_kind(), Tok::Newline | Tok::BlankLine) {
            self.bump();
        }
    }

    fn recover_to_newline(&mut self) {
        loop {
            match self.peek_kind() {
                Tok::Newline | Tok::BlankLine | Tok::Eof => break,
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn peek_kind(&self) -> &Tok {
        &self.tokens[self.pos].tok
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn last_span(&self) -> Span {
        if self.pos == 0 {
            self.tokens[0].span
        } else {
            self.tokens[self.pos - 1].span
        }
    }

    fn bump(&mut self) {
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, expected: Tok) -> Result<(), ()> {
        if std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(&expected) {
            self.bump();
            Ok(())
        } else {
            let span = self.peek_span();
            self.diags.push(ParseDiag {
                message: format!(
                    "expected {}, got {}",
                    describe_tok(&expected),
                    describe_tok(self.peek_kind())
                ),
                span,
            });
            Err(())
        }
    }

    fn expect_ident(&mut self) -> Result<(String, Span), ()> {
        let span = self.peek_span();
        if let Tok::Ident(s) = self.peek_kind().clone() {
            self.bump();
            Ok((s, span))
        } else {
            self.diags.push(ParseDiag {
                message: format!(
                    "expected identifier, got {}",
                    describe_tok(self.peek_kind())
                ),
                span,
            });
            Err(())
        }
    }
}

fn describe_tok(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => format!("identifier `{}`", s),
        Tok::KwNamespace => "`namespace`".into(),
        Tok::KwEnum => "`enum`".into(),
        Tok::KwType => "`type`".into(),
        Tok::KwUse => "`use`".into(),
        Tok::KwTrue => "`true`".into(),
        Tok::KwFalse => "`false`".into(),
        Tok::KwNone => "`none`".into(),
        Tok::Int { .. } => "integer literal".into(),
        Tok::Float { .. } => "float literal".into(),
        Tok::Str(_) => "string literal".into(),
        Tok::Eq => "`=`".into(),
        Tok::Colon => "`:`".into(),
        Tok::ColonColon => "`::`".into(),
        Tok::Comma => "`,`".into(),
        Tok::LBrace => "`{`".into(),
        Tok::RBrace => "`}`".into(),
        Tok::LBracket => "`[`".into(),
        Tok::RBracket => "`]`".into(),
        Tok::LParen => "`(`".into(),
        Tok::RParen => "`)`".into(),
        Tok::Lt => "`<`".into(),
        Tok::Gt => "`>`".into(),
        Tok::Question => "`?`".into(),
        Tok::At => "`@`".into(),
        Tok::Minus => "`-`".into(),
        Tok::LineComment(_) => "line comment".into(),
        Tok::DocComment(_) => "doc comment".into(),
        Tok::FileDocComment(_) => "file doc comment".into(),
        Tok::Newline => "newline".into(),
        Tok::BlankLine => "blank line".into(),
        Tok::Eof => "end of file".into(),
    }
}
