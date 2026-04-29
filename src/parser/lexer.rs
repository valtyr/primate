//! Lexer for the primate DSL.
//!
//! Produces a stream of tokens with source spans. All trivia (comments,
//! blank lines) is preserved as tokens so the formatter can reconstruct
//! the canonical layout.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub column: u32,
}

impl Span {
    pub fn len(&self) -> u32 {
        (self.end - self.start) as u32
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    // Identifiers and keywords
    Ident(String),
    KwNamespace,
    KwEnum,
    KwType,
    KwUse,
    KwTrue,
    KwFalse,
    KwNone,

    // Literals
    Int { value: i128, suffix: Option<String> },
    Float { value: f64, suffix: Option<String> },
    Str(String),

    // Punctuation
    Eq,         // =
    Colon,      // :
    ColonColon, // ::
    Comma,      // ,
    LBrace,     // {
    RBrace,     // }
    LBracket,   // [
    RBracket,   // ]
    LParen,     // (
    RParen,     // )
    Lt,         // <
    Gt,         // >
    Question,   // ?
    At,         // @
    Minus,      // -

    // Trivia
    LineComment(String),    // //   (text without leading slashes)
    DocComment(String),     // ///  (text without leading slashes, leading space trimmed)
    FileDocComment(String), // //!  (text without leading slashes/bang)
    Newline,                // physical newline (significant for declaration boundaries)
    BlankLine,              // emitted once for each run of >1 newlines (formatting cue)

    Eof,
}

impl Tok {
    pub fn is_trivia(&self) -> bool {
        matches!(
            self,
            Tok::LineComment(_)
                | Tok::DocComment(_)
                | Tok::FileDocComment(_)
                | Tok::Newline
                | Tok::BlankLine
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

#[derive(Debug, thiserror::Error)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

pub struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: u32,
    column: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    pub fn lex_all(mut self) -> (Vec<Token>, Vec<LexError>) {
        let mut tokens = Vec::new();
        let mut errors = Vec::new();
        loop {
            match self.next_token() {
                Ok(token) => {
                    let is_eof = matches!(token.tok, Tok::Eof);
                    tokens.push(token);
                    if is_eof {
                        break;
                    }
                }
                Err(e) => {
                    errors.push(e);
                    // Recover: skip a byte and continue
                    self.advance_byte();
                }
            }
        }
        (tokens, errors)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn advance_byte(&mut self) {
        if let Some(b) = self.peek() {
            self.pos += 1;
            if b == b'\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
    }

    fn make_span(&self, start: usize, start_line: u32, start_col: u32) -> Span {
        Span {
            start,
            end: self.pos,
            line: start_line,
            column: start_col,
        }
    }

    fn next_token(&mut self) -> Result<Token, LexError> {
        // Skip horizontal whitespace (not newlines)
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\r' {
                self.advance_byte();
            } else {
                break;
            }
        }

        let start = self.pos;
        let start_line = self.line;
        let start_col = self.column;

        let b = match self.peek() {
            Some(b) => b,
            None => {
                return Ok(Token {
                    tok: Tok::Eof,
                    span: self.make_span(start, start_line, start_col),
                });
            }
        };

        // Newlines (one Newline for isolated; BlankLine when 2+ in a row).
        if b == b'\n' {
            self.advance_byte();
            let mut saw_blank = false;
            loop {
                let saved_pos = self.pos;
                let saved_line = self.line;
                let saved_col = self.column;
                while let Some(c) = self.peek() {
                    if c == b' ' || c == b'\t' || c == b'\r' {
                        self.advance_byte();
                    } else {
                        break;
                    }
                }
                if self.peek() == Some(b'\n') {
                    saw_blank = true;
                    self.advance_byte();
                } else {
                    // Restore state so the consumer sees the whitespace untouched.
                    self.pos = saved_pos;
                    self.line = saved_line;
                    self.column = saved_col;
                    break;
                }
            }
            let tok = if saw_blank {
                Tok::BlankLine
            } else {
                Tok::Newline
            };
            return Ok(Token {
                tok,
                span: self.make_span(start, start_line, start_col),
            });
        }

        // Comments
        if b == b'/' && self.peek_at(1) == Some(b'/') {
            return Ok(self.lex_comment(start, start_line, start_col));
        }

        // Block comments are explicitly forbidden (RFC 0002)
        if b == b'/' && self.peek_at(1) == Some(b'*') {
            self.advance_byte();
            self.advance_byte();
            return Err(LexError {
                message: "block comments (/* */) are not supported; use // or ///".to_string(),
                span: self.make_span(start, start_line, start_col),
            });
        }

        // String literals (regular and raw)
        if b == b'"' {
            return self.lex_string(start, start_line, start_col, false, 0);
        }
        if b == b'r' && (self.peek_at(1) == Some(b'"') || self.peek_at(1) == Some(b'#')) {
            // Raw string: r"..." or r#"..."#  (count #s)
            self.advance_byte(); // consume r
            let mut hashes = 0;
            while self.peek() == Some(b'#') {
                self.advance_byte();
                hashes += 1;
            }
            if self.peek() != Some(b'"') {
                return Err(LexError {
                    message: "expected '\"' after raw string prefix".to_string(),
                    span: self.make_span(start, start_line, start_col),
                });
            }
            return self.lex_string(start, start_line, start_col, true, hashes);
        }

        // Numbers (and negative numbers handled in parser; here `-` is just a token)
        if b.is_ascii_digit() {
            return self.lex_number(start, start_line, start_col);
        }

        // Identifiers / keywords
        if b == b'_' || b.is_ascii_alphabetic() {
            return Ok(self.lex_ident(start, start_line, start_col));
        }

        // Punctuation
        let tok = match b {
            b'=' => {
                self.advance_byte();
                Tok::Eq
            }
            b':' => {
                self.advance_byte();
                if self.peek() == Some(b':') {
                    self.advance_byte();
                    Tok::ColonColon
                } else {
                    Tok::Colon
                }
            }
            b',' => {
                self.advance_byte();
                Tok::Comma
            }
            b'{' => {
                self.advance_byte();
                Tok::LBrace
            }
            b'}' => {
                self.advance_byte();
                Tok::RBrace
            }
            b'[' => {
                self.advance_byte();
                Tok::LBracket
            }
            b']' => {
                self.advance_byte();
                Tok::RBracket
            }
            b'(' => {
                self.advance_byte();
                Tok::LParen
            }
            b')' => {
                self.advance_byte();
                Tok::RParen
            }
            b'<' => {
                self.advance_byte();
                Tok::Lt
            }
            b'>' => {
                self.advance_byte();
                Tok::Gt
            }
            b'?' => {
                self.advance_byte();
                Tok::Question
            }
            b'@' => {
                self.advance_byte();
                Tok::At
            }
            b'-' => {
                self.advance_byte();
                Tok::Minus
            }
            b';' => {
                self.advance_byte();
                return Err(LexError {
                    message: "semicolons are not used in primate; remove the ';'".to_string(),
                    span: self.make_span(start, start_line, start_col),
                });
            }
            other => {
                self.advance_byte();
                return Err(LexError {
                    message: format!("unexpected character {:?}", other as char),
                    span: self.make_span(start, start_line, start_col),
                });
            }
        };

        Ok(Token {
            tok,
            span: self.make_span(start, start_line, start_col),
        })
    }

    fn lex_comment(&mut self, start: usize, start_line: u32, start_col: u32) -> Token {
        // We know we have `//` ahead.
        self.advance_byte(); // /
        self.advance_byte(); // /

        let mut kind = 0; // 0 = line, 1 = doc (///), 2 = file doc (//!)
        if self.peek() == Some(b'/') {
            // Could be `///` (doc) or `////...` (still line comment per Rust convention).
            // Rust treats /// as doc and //// as plain line. We'll do the same.
            if self.peek_at(1) != Some(b'/') {
                self.advance_byte();
                kind = 1;
            }
        } else if self.peek() == Some(b'!') {
            self.advance_byte();
            kind = 2;
        }

        // Optional single space after the marker is part of formatting, we trim it.
        if self.peek() == Some(b' ') {
            self.advance_byte();
        }

        let text_start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'\n' {
                break;
            }
            self.advance_byte();
        }
        let text = self.src[text_start..self.pos].trim_end().to_string();

        let tok = match kind {
            1 => Tok::DocComment(text),
            2 => Tok::FileDocComment(text),
            _ => Tok::LineComment(text),
        };
        Token {
            tok,
            span: self.make_span(start, start_line, start_col),
        }
    }

    fn lex_string(
        &mut self,
        start: usize,
        start_line: u32,
        start_col: u32,
        raw: bool,
        hashes: usize,
    ) -> Result<Token, LexError> {
        // Opening quote
        self.advance_byte();

        let mut value = String::new();

        loop {
            let c = match self.peek() {
                Some(c) => c,
                None => {
                    return Err(LexError {
                        message: "unterminated string literal".to_string(),
                        span: self.make_span(start, start_line, start_col),
                    });
                }
            };

            if raw {
                if c == b'"' {
                    // Check for matching number of `#`s
                    let mut ok = true;
                    for i in 0..hashes {
                        if self.peek_at(1 + i) != Some(b'#') {
                            ok = false;
                            break;
                        }
                    }
                    if ok {
                        self.advance_byte(); // closing "
                        for _ in 0..hashes {
                            self.advance_byte();
                        }
                        return Ok(Token {
                            tok: Tok::Str(value),
                            span: self.make_span(start, start_line, start_col),
                        });
                    }
                }
                if c == b'\n' {
                    self.advance_byte();
                    value.push('\n');
                } else {
                    let ch = self.src[self.pos..]
                        .chars()
                        .next()
                        .expect("non-empty bytes implies at least one char");
                    let ch_len = ch.len_utf8();
                    for _ in 0..ch_len {
                        self.advance_byte();
                    }
                    value.push(ch);
                }
            } else {
                if c == b'"' {
                    self.advance_byte();
                    return Ok(Token {
                        tok: Tok::Str(value),
                        span: self.make_span(start, start_line, start_col),
                    });
                }
                if c == b'\\' {
                    self.advance_byte();
                    let escape = match self.peek() {
                        Some(b'n') => '\n',
                        Some(b't') => '\t',
                        Some(b'r') => '\r',
                        Some(b'\\') => '\\',
                        Some(b'"') => '"',
                        Some(b'0') => '\0',
                        Some(other) => {
                            return Err(LexError {
                                message: format!("invalid escape sequence \\{}", other as char),
                                span: self.make_span(start, start_line, start_col),
                            });
                        }
                        None => {
                            return Err(LexError {
                                message: "unterminated string after backslash".to_string(),
                                span: self.make_span(start, start_line, start_col),
                            });
                        }
                    };
                    self.advance_byte();
                    value.push(escape);
                    continue;
                }
                if c == b'\n' {
                    return Err(LexError {
                        message: "newline in string literal; use \\n or a raw string r\"...\""
                            .to_string(),
                        span: self.make_span(start, start_line, start_col),
                    });
                }
                let ch = self.src[self.pos..]
                    .chars()
                    .next()
                    .expect("non-empty bytes implies at least one char");
                let ch_len = ch.len_utf8();
                for _ in 0..ch_len {
                    self.advance_byte();
                }
                value.push(ch);
            }
        }
    }

    fn lex_number(
        &mut self,
        start: usize,
        start_line: u32,
        start_col: u32,
    ) -> Result<Token, LexError> {
        // Hex / binary / octal: must start with 0x/0b/0o, no suffix allowed
        if self.peek() == Some(b'0') {
            match self.peek_at(1) {
                Some(b'x') | Some(b'X') => {
                    self.advance_byte();
                    self.advance_byte();
                    return self.lex_radix_int(start, start_line, start_col, 16, "0x");
                }
                Some(b'b') | Some(b'B') => {
                    self.advance_byte();
                    self.advance_byte();
                    return self.lex_radix_int(start, start_line, start_col, 2, "0b");
                }
                Some(b'o') | Some(b'O') => {
                    self.advance_byte();
                    self.advance_byte();
                    return self.lex_radix_int(start, start_line, start_col, 8, "0o");
                }
                _ => {}
            }
        }

        // Decimal integer / float
        let mut digits = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                digits.push(c as char);
                self.advance_byte();
            } else if c == b'_' {
                self.advance_byte(); // skip separator
            } else {
                break;
            }
        }

        let mut is_float = false;
        // Decimal point
        if self.peek() == Some(b'.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            digits.push('.');
            self.advance_byte();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    digits.push(c as char);
                    self.advance_byte();
                } else if c == b'_' {
                    self.advance_byte();
                } else {
                    break;
                }
            }
        }

        // Exponent
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_float = true;
            digits.push('e');
            self.advance_byte();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                digits.push(self.peek().unwrap() as char);
                self.advance_byte();
            }
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    digits.push(c as char);
                    self.advance_byte();
                } else if c == b'_' {
                    self.advance_byte();
                } else {
                    break;
                }
            }
        }

        // Optional unit suffix: either an alphabetic identifier directly
        // attached (e.g. `30s`, `100MiB`, `1w`) or a single `%` (for
        // percentage on floats). Reserved unit-style suffixes are handled in
        // the parser; the lexer simply captures them.
        let suffix_start = self.pos;
        if self.peek() == Some(b'%') {
            self.advance_byte();
        } else {
            while let Some(c) = self.peek() {
                if c.is_ascii_alphabetic() {
                    self.advance_byte();
                } else {
                    break;
                }
            }
        }
        let suffix = if self.pos > suffix_start {
            Some(self.src[suffix_start..self.pos].to_string())
        } else {
            None
        };

        if is_float {
            let value: f64 = digits.parse().map_err(|_| LexError {
                message: format!("invalid float literal '{}'", digits),
                span: self.make_span(start, start_line, start_col),
            })?;
            Ok(Token {
                tok: Tok::Float { value, suffix },
                span: self.make_span(start, start_line, start_col),
            })
        } else {
            let value: i128 = digits.parse().map_err(|_| LexError {
                message: format!("invalid integer literal '{}'", digits),
                span: self.make_span(start, start_line, start_col),
            })?;
            Ok(Token {
                tok: Tok::Int { value, suffix },
                span: self.make_span(start, start_line, start_col),
            })
        }
    }

    fn lex_radix_int(
        &mut self,
        start: usize,
        start_line: u32,
        start_col: u32,
        radix: u32,
        prefix: &str,
    ) -> Result<Token, LexError> {
        let mut digits = String::new();
        while let Some(c) = self.peek() {
            let valid = match radix {
                16 => c.is_ascii_hexdigit(),
                2 => c == b'0' || c == b'1',
                8 => (b'0'..=b'7').contains(&c),
                _ => unreachable!(),
            };
            if valid {
                digits.push(c as char);
                self.advance_byte();
            } else if c == b'_' {
                self.advance_byte();
            } else {
                break;
            }
        }

        // Reject suffix on radix literals
        if matches!(self.peek(), Some(c) if c.is_ascii_alphabetic()) {
            return Err(LexError {
                message: format!("unit suffixes are not allowed on {} literals", prefix),
                span: self.make_span(start, start_line, start_col),
            });
        }

        if digits.is_empty() {
            return Err(LexError {
                message: format!("expected digits after '{}'", prefix),
                span: self.make_span(start, start_line, start_col),
            });
        }

        let value = i128::from_str_radix(&digits, radix).map_err(|_| LexError {
            message: format!("invalid {} literal '{}{}'", prefix, prefix, digits),
            span: self.make_span(start, start_line, start_col),
        })?;
        Ok(Token {
            tok: Tok::Int {
                value,
                suffix: None,
            },
            span: self.make_span(start, start_line, start_col),
        })
    }

    fn lex_ident(&mut self, start: usize, start_line: u32, start_col: u32) -> Token {
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.advance_byte();
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];

        let tok = match text {
            "namespace" => Tok::KwNamespace,
            "enum" => Tok::KwEnum,
            "type" => Tok::KwType,
            "use" => Tok::KwUse,
            "true" => Tok::KwTrue,
            "false" => Tok::KwFalse,
            "none" => Tok::KwNone,
            _ => Tok::Ident(text.to_string()),
        };
        Token {
            tok,
            span: self.make_span(start, start_line, start_col),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_ok(src: &str) -> Vec<Tok> {
        let (tokens, errors) = Lexer::new(src).lex_all();
        assert!(errors.is_empty(), "unexpected lex errors: {:?}", errors);
        tokens.into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn simple_decl() {
        let toks = lex_ok("u32 MAX = 8\n");
        assert!(matches!(toks[0], Tok::Ident(ref s) if s == "u32"));
        assert!(matches!(toks[1], Tok::Ident(ref s) if s == "MAX"));
        assert!(matches!(toks[2], Tok::Eq));
        assert!(matches!(toks[3], Tok::Int { value: 8, .. }));
        assert!(matches!(toks[4], Tok::Newline));
    }

    #[test]
    fn duration_suffix() {
        let toks = lex_ok("duration TIMEOUT = 30s\n");
        assert!(matches!(&toks[3], Tok::Int { value: 30, suffix: Some(s) } if s == "s"));
    }

    #[test]
    fn doc_and_file_doc() {
        let toks = lex_ok("//! file\n/// doc\n// line\n");
        assert!(matches!(&toks[0], Tok::FileDocComment(s) if s == "file"));
        assert!(matches!(&toks[2], Tok::DocComment(s) if s == "doc"));
        assert!(matches!(&toks[4], Tok::LineComment(s) if s == "line"));
    }

    #[test]
    fn hex_no_suffix() {
        let toks = lex_ok("0xFF\n");
        assert!(matches!(
            toks[0],
            Tok::Int {
                value: 255,
                suffix: None
            }
        ));
    }

    #[test]
    fn paths_and_punct() {
        let toks = lex_ok("core::time::Foo\n");
        assert!(matches!(&toks[0], Tok::Ident(s) if s == "core"));
        assert!(matches!(toks[1], Tok::ColonColon));
        assert!(matches!(&toks[2], Tok::Ident(s) if s == "time"));
        assert!(matches!(toks[3], Tok::ColonColon));
        assert!(matches!(&toks[4], Tok::Ident(s) if s == "Foo"));
    }

    #[test]
    fn blank_line_distinguished() {
        let toks = lex_ok("a\n\nb\n");
        // a, BlankLine, b, Newline
        assert!(matches!(&toks[0], Tok::Ident(s) if s == "a"));
        assert!(matches!(toks[1], Tok::BlankLine));
        assert!(matches!(&toks[2], Tok::Ident(s) if s == "b"));
        assert!(matches!(toks[3], Tok::Newline));
    }

    #[test]
    fn raw_string() {
        let toks = lex_ok(r####"r#"with "quotes""#"####);
        assert!(matches!(&toks[0], Tok::Str(s) if s == "with \"quotes\""));
    }

    #[test]
    fn semicolon_rejected() {
        let (_, errors) = Lexer::new("u32 X = 1;\n").lex_all();
        assert!(errors.iter().any(|e| e.message.contains("semicolons")));
    }

    #[test]
    fn block_comment_rejected() {
        let (_, errors) = Lexer::new("/* hi */\n").lex_all();
        assert!(errors.iter().any(|e| e.message.contains("block comments")));
    }
}
