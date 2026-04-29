//! Parser for the primate DSL (`.prim` files).
//!
//! Pipeline: source -> tokens -> AST -> IR. Cross-file resolution happens
//! during the AST -> IR lowering pass.

pub mod ast;
pub mod grammar;
pub mod lexer;
pub mod lower;

use crate::diagnostics::{Diagnostic, Diagnostics, Severity};
use crate::ir::{EnumDef, Module, TypeAliasDef};
use std::path::Path;
use walkdir::WalkDir;

pub use lower::ParsedFile;

/// A discovered `.prim` file with its derived namespace.
#[derive(Debug)]
pub struct ConstFile {
    pub path: std::path::PathBuf,
    /// Namespace derived from the directory layout. May be overridden by a
    /// top-of-file `namespace` declaration.
    pub namespace: String,
    pub content: String,
}

#[derive(Debug, Default)]
pub struct ParsedProject {
    pub modules: Vec<Module>,
    pub enums: Vec<EnumDef>,
    pub aliases: Vec<TypeAliasDef>,
    pub diagnostics: Diagnostics,
}

/// Discover all `.prim` files in a directory tree.
pub fn discover_files(input_dir: &Path) -> Result<Vec<ConstFile>, DiscoverError> {
    let mut files = Vec::new();

    for entry in WalkDir::new(input_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("prim") {
            continue;
        }
        let namespace = derive_namespace(input_dir, path);
        let content = std::fs::read_to_string(path)?;
        files.push(ConstFile {
            path: path.to_path_buf(),
            namespace,
            content,
        });
    }

    Ok(files)
}

/// Derive a `::`-separated namespace from a file's path relative to the input root.
fn derive_namespace(input_dir: &Path, file_path: &Path) -> String {
    let relative = file_path.strip_prefix(input_dir).unwrap_or(file_path);
    let mut parts: Vec<String> = relative
        .parent()
        .map(|p| {
            p.iter()
                .filter_map(|c| c.to_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
        parts.push(stem.to_string());
    }
    parts.join("::")
}

/// Parse a single source file into an AST. Lex/parse diagnostics are
/// returned alongside the AST.
pub fn parse_source(content: &str, file: &Path) -> (ast::File, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let (tokens, lex_errors) = lexer::Lexer::new(content).lex_all();
    for err in lex_errors {
        diagnostics.add(Diagnostic {
            file: file.display().to_string(),
            line: err.span.line,
            column: err.span.column,
            length: Some(err.span.len()),
            severity: Severity::Error,
            code: "parse-error".to_string(),
            message: err.message,
            targets: vec![],
        });
    }
    let mut parser = grammar::Parser::new(tokens);
    let ast = parser.parse_file();
    for diag in parser.diags {
        diagnostics.add(Diagnostic {
            file: file.display().to_string(),
            line: diag.span.line,
            column: diag.span.column,
            length: Some(diag.span.len()),
            severity: Severity::Error,
            code: "parse-error".to_string(),
            message: diag.message,
            targets: vec![],
        });
    }
    (ast, diagnostics)
}

/// Parse and lower an entire project, performing cross-file type resolution.
pub fn parse_project(files: Vec<ConstFile>) -> ParsedProject {
    let mut diagnostics = Diagnostics::new();
    let mut parsed_files = Vec::new();

    for file in files {
        let (ast, diags) = parse_source(&file.content, &file.path);
        for d in diags.diagnostics {
            diagnostics.add(d);
        }
        parsed_files.push(ParsedFile {
            path: file.path,
            default_namespace: file.namespace,
            ast,
            source_text: file.content,
        });
    }

    let resolved = lower::lower(parsed_files);
    for d in resolved.diagnostics.diagnostics {
        diagnostics.add(d);
    }

    ParsedProject {
        modules: resolved.modules,
        enums: resolved.enums,
        aliases: resolved.aliases,
        diagnostics,
    }
}

/// Parse a single file and lower it as a one-file project. Used by the LSP
/// to surface diagnostics for a buffer in isolation.
pub fn parse_file(file: &ConstFile) -> ParsedProject {
    parse_project(vec![ConstFile {
        path: file.path.clone(),
        namespace: file.namespace.clone(),
        content: file.content.clone(),
    }])
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("failed to read file: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(name: &str, namespace: &str, content: &str) -> ParsedProject {
        let file = ConstFile {
            path: std::path::PathBuf::from(name),
            namespace: namespace.to_string(),
            content: content.to_string(),
        };
        parse_project(vec![file])
    }

    #[test]
    fn simple_const() {
        let p = parse_one("test.prim", "test", "u32 MAX_USERS = 8\n");
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
        assert_eq!(p.modules.len(), 1);
        assert_eq!(p.modules[0].constants.len(), 1);
        assert_eq!(p.modules[0].constants[0].name, "MAX_USERS");
    }

    #[test]
    fn duration_with_suffix() {
        let p = parse_one("t.prim", "t", "duration TIMEOUT = 30s\n");
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    #[test]
    fn enum_top_level() {
        let p = parse_one(
            "t.prim",
            "t",
            "enum Status {\n    Pending,\n    Active,\n    Done,\n}\n",
        );
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
        assert_eq!(p.enums.len(), 1);
    }

    #[test]
    fn enum_int_backed() {
        let p = parse_one(
            "t.prim",
            "t",
            "enum LogLevel: u8 {\n    Debug = 0,\n    Info = 1,\n}\n",
        );
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
        assert_eq!(p.enums[0].backing_type, "integer");
    }

    #[test]
    fn type_alias() {
        let p = parse_one("t.prim", "t", "type Port = u32\nPort HTTP_PORT = 8080\n");
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
        assert_eq!(p.aliases.len(), 1);
    }

    #[test]
    fn doc_attaches_to_decl() {
        let p = parse_one("t.prim", "t", "/// Maximum users.\nu32 MAX_USERS = 8\n");
        assert!(!p.diagnostics.has_errors());
        assert_eq!(
            p.modules[0].constants[0].doc.as_deref(),
            Some("Maximum users.")
        );
    }

    #[test]
    fn rejects_inferred_typing() {
        let p = parse_one("t.prim", "t", "MAX = 8\n");
        assert!(p.diagnostics.has_errors());
    }

    fn has_diag(p: &ParsedProject, code: &str) -> bool {
        p.diagnostics.diagnostics.iter().any(|d| d.code == code)
    }

    #[test]
    fn out_of_range_i32_overflow() {
        // RFC 0004 §3: a literal too big for the declared type is an error.
        let p = parse_one("t.prim", "t", "i32 X = 3_000_000_000\n");
        assert!(has_diag(&p, "out-of-range"), "{:?}", p.diagnostics);
    }

    #[test]
    fn out_of_range_u32_negative() {
        // Negative on unsigned is now subsumed by out-of-range.
        let p = parse_one("t.prim", "t", "u32 X = -1\n");
        assert!(has_diag(&p, "out-of-range"), "{:?}", p.diagnostics);
    }

    #[test]
    fn out_of_range_u64_max_is_accepted() {
        // u64::MAX must be representable post-RFC4 i128 widening.
        let p = parse_one("t.prim", "t", "u64 X = 18_446_744_073_709_551_615\n");
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    #[test]
    fn out_of_range_enum_variant_overflow_backing() {
        // u8 backing has range 0..=255 — variant value 300 overflows.
        let p = parse_one(
            "t.prim",
            "t",
            "enum Big: u8 {\n    A = 0,\n    B = 300,\n}\n",
        );
        assert!(has_diag(&p, "out-of-range"), "{:?}", p.diagnostics);
    }

    #[test]
    fn week_suffix_on_duration() {
        let p = parse_one("t.prim", "t", "duration TTL = 2w\n");
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
        // 2 weeks in nanoseconds.
        let nanos = 2u64 * 7 * 24 * 60 * 60 * 1_000_000_000;
        match &p.modules[0].constants[0].value {
            crate::types::Value::Duration { nanoseconds } => assert_eq!(*nanoseconds, nanos),
            other => panic!("expected duration, got {:?}", other),
        }
    }

    #[test]
    fn percent_suffix_on_float() {
        let p = parse_one("t.prim", "t", "f64 ROLLOUT = 5%\nf64 OPACITY = 12.5%\n");
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
        match &p.modules[0].constants[0].value {
            crate::types::Value::Float(v) => assert!((v - 0.05).abs() < 1e-9, "got {}", v),
            other => panic!("expected float, got {:?}", other),
        }
        match &p.modules[0].constants[1].value {
            crate::types::Value::Float(v) => assert!((v - 0.125).abs() < 1e-9, "got {}", v),
            other => panic!("expected float, got {:?}", other),
        }
    }

    #[test]
    fn percent_suffix_on_integer_type_is_rejected() {
        // `%` only makes sense on float types.
        let p = parse_one("t.prim", "t", "u32 X = 50%\n");
        assert!(p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    #[test]
    fn byte_suffix_on_integer_literal() {
        // RFC 0004 §1: byte suffixes are sugar on integer literals.
        let p = parse_one("t.prim", "t", "u64 X = 100MiB\n");
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    #[test]
    fn byte_suffix_overflowing_target_type_is_out_of_range() {
        // 5GiB > u32::MAX → out-of-range against the declared u32.
        let p = parse_one("t.prim", "t", "u32 X = 5GiB\n");
        assert!(has_diag(&p, "out-of-range"), "{:?}", p.diagnostics);
    }

    #[test]
    fn duration_suffix_on_integer_is_an_error() {
        // RFC 0004 §1: only byte suffixes are valid on integer literals.
        let p = parse_one("t.prim", "t", "u64 X = 30s\n");
        assert!(p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    #[test]
    fn bytes_type_is_no_longer_recognized() {
        // RFC 0004 §1: `bytes` is dropped as a type.
        let p = parse_one("t.prim", "t", "bytes X = 100MiB\n");
        assert!(p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    #[test]
    fn enum_variant_within_backing_is_accepted() {
        let p = parse_one(
            "t.prim",
            "t",
            "enum Lvl: u8 {\n    Debug = 0,\n    Error = 255,\n}\n",
        );
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    #[test]
    fn multiline_tuple_type_is_accepted() {
        // RFC 0003 §4: newlines insignificant inside <>.
        let p = parse_one(
            "t.prim",
            "t",
            "type Triple = tuple<\n    u32,\n    u32,\n    u32,\n>\n",
        );
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
        assert_eq!(p.aliases.len(), 1);
    }

    #[test]
    fn multiline_map_type_is_accepted() {
        let p = parse_one("t.prim", "t", "type Cfg = map<\n    string,\n    u32,\n>\n");
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    fn parse_two(
        a_path: &str,
        a_ns: &str,
        a_src: &str,
        b_path: &str,
        b_ns: &str,
        b_src: &str,
    ) -> ParsedProject {
        let files = vec![
            ConstFile {
                path: std::path::PathBuf::from(a_path),
                namespace: a_ns.to_string(),
                content: a_src.to_string(),
            },
            ConstFile {
                path: std::path::PathBuf::from(b_path),
                namespace: b_ns.to_string(),
                content: b_src.to_string(),
            },
        ];
        parse_project(files)
    }

    #[test]
    fn use_single_form_resolves_imported_type() {
        // RFC 0003 §3 — `use net::Port` lets the consumer file reference `Port` by bare name.
        let p = parse_two(
            "net.prim",
            "net",
            "type Port = u32\n",
            "app.prim",
            "app",
            "use net::Port\nPort HTTP_PORT = 8080\n",
        );
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    #[test]
    fn use_brace_form_resolves_each_imported_name() {
        let p = parse_two(
            "net.prim",
            "net",
            "type Port = u32\ntype IP = string\n",
            "app.prim",
            "app",
            "use net::{Port, IP}\nPort PORT = 8080\nIP HOST = \"localhost\"\n",
        );
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    #[test]
    fn unresolved_import_is_an_error() {
        let p = parse_two(
            "net.prim",
            "net",
            "type Port = u32\n",
            "app.prim",
            "app",
            "use net::Missing\n",
        );
        assert!(p.diagnostics.has_errors());
        assert!(
            p.diagnostics
                .diagnostics
                .iter()
                .any(|d| d.code == "unresolved-import"),
            "{:?}",
            p.diagnostics
        );
    }

    #[test]
    fn tuple_value_with_square_brackets() {
        // RFC 0003 §1: tuple values use `[...]`.
        let p = parse_one(
            "t.prim",
            "t",
            "type Color = tuple<u32, u32, u32>\nColor BLACK = [0, 0, 0]\n",
        );
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    #[test]
    fn old_paren_tuple_value_is_rejected() {
        let p = parse_one(
            "t.prim",
            "t",
            "type Color = tuple<u32, u32, u32>\nColor BLACK = (0, 0, 0)\n",
        );
        assert!(p.diagnostics.has_errors());
        assert!(
            p.diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("tuple values use `[...]`")),
            "expected migration message, got {:?}",
            p.diagnostics
        );
    }

    #[test]
    fn fixed_size_array_type_parses_and_resolves() {
        // RFC 0003 §2: array<T, N> for fixed-size homogeneous arrays.
        let p = parse_one(
            "t.prim",
            "t",
            "type Pixel = array<u32, 3>\nPixel WHITE = [255, 255, 255]\n",
        );
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
        assert_eq!(p.aliases.len(), 1);
    }

    #[test]
    fn fixed_size_array_reports_both_length_and_element_errors() {
        // Wrong length AND wrong element type — both diagnostics should fire.
        let p = parse_one(
            "t.prim",
            "t",
            "type Triple = array<u32, 3>\nTriple BAD = [\"h\"]\n",
        );
        assert!(p.diagnostics.has_errors());
        let codes: Vec<&str> = p
            .diagnostics
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"length-mismatch"),
            "expected length-mismatch in {:?}",
            codes
        );
        assert!(
            codes.contains(&"type-mismatch"),
            "expected type-mismatch in {:?}",
            codes
        );
    }

    #[test]
    fn fixed_size_array_length_mismatch_is_an_error() {
        let p = parse_one(
            "t.prim",
            "t",
            "type Pixel = array<u32, 3>\nPixel SHORT = [1, 2]\n",
        );
        assert!(p.diagnostics.has_errors());
        assert!(
            p.diagnostics
                .diagnostics
                .iter()
                .any(|d| d.code == "length-mismatch"),
            "{:?}",
            p.diagnostics
        );
    }

    #[test]
    fn fixed_size_array_nested_inside_array() {
        // Matrix: array<array<u32, 3>, 3>.
        let p = parse_one(
            "t.prim",
            "t",
            "type Pixel = array<u32, 3>\ntype Matrix = array<Pixel, 3>\nMatrix IDENTITY = [[1,0,0], [0,1,0], [0,0,1]]\n",
        );
        assert!(!p.diagnostics.has_errors(), "{:?}", p.diagnostics);
    }

    #[test]
    fn import_collides_with_local_decl() {
        let p = parse_two(
            "net.prim",
            "net",
            "type Port = u32\n",
            "app.prim",
            "app",
            "use net::Port\ntype Port = u32\n",
        );
        assert!(p.diagnostics.has_errors());
        assert!(
            p.diagnostics
                .diagnostics
                .iter()
                .any(|d| d.code == "import-collision"),
            "{:?}",
            p.diagnostics
        );
    }
}
