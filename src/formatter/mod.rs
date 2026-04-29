//! Canonical formatter for `.prim` files.
//!
//! Walks the AST and emits the file in canonical form (4-space indent, single
//! space around `=` and after `:`, trailing commas in multi-line collections).
//! Within groups of consecutive declarations (no blank line between), the
//! type, name, and `=` columns are aligned.

use crate::parser::ast::*;
use crate::parser::{lexer, parse_source};
use crate::diagnostics::Diagnostics;

const INDENT: &str = "    ";

/// Soft column budget for line wrapping (RFC 0003 §4). Lines longer than
/// this are wrapped at the shallowest fitting collection delimiter.
const COLUMN_BUDGET: usize = 100;

/// Format the given source. On parse error, returns the diagnostics; the
/// formatter refuses to rewrite a file it can't parse cleanly.
pub fn format_source(source: &str) -> Result<String, Diagnostics> {
    let path = std::path::PathBuf::from("<formatter>");
    let (ast, diags) = parse_source(source, &path);
    if diags.has_errors() {
        return Err(diags);
    }
    Ok(format_ast(&ast))
}

pub fn format_ast(file: &File) -> String {
    let mut out = String::new();
    let groups = group_items(&file.items);

    let mut prev_kind: Option<&'static str> = None;
    for group in &groups {
        match group {
            Group::Blank => {
                // Avoid emitting more than one trailing blank line.
                if !out.ends_with("\n\n") {
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push('\n');
                }
                prev_kind = Some("blank");
            }
            Group::FileDoc(text) => {
                out.push_str(&format!("//! {}\n", text.trim()));
                prev_kind = Some("filedoc");
            }
            Group::LineComment(text) => {
                out.push_str(&format!("//{}\n", normalize_comment_text(text)));
                prev_kind = Some("comment");
            }
            Group::Decls(decls) => {
                format_decl_group(decls, &mut out);
                prev_kind = Some("decls");
            }
            Group::Uses(uses) => {
                format_use_group(uses, &mut out);
                prev_kind = Some("uses");
            }
        }
    }

    // Ensure trailing newline.
    if !out.ends_with('\n') {
        out.push('\n');
    }
    // Trim trailing blank lines down to a single newline.
    while out.ends_with("\n\n") {
        out.pop();
    }
    let _ = prev_kind;
    out
}

fn normalize_comment_text(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        format!(" {}", s.trim())
    }
}

#[derive(Debug)]
enum Group<'a> {
    Blank,
    FileDoc(String),
    LineComment(String),
    Decls(Vec<&'a Decl>),
    Uses(Vec<&'a UseDecl>),
}

fn group_items<'a>(items: &'a [Item]) -> Vec<Group<'a>> {
    let mut groups: Vec<Group<'a>> = Vec::new();
    let mut current_decls: Vec<&'a Decl> = Vec::new();
    let mut current_uses: Vec<&'a UseDecl> = Vec::new();

    let flush_decls =
        |current: &mut Vec<&'a Decl>, groups: &mut Vec<Group<'a>>| {
            if !current.is_empty() {
                groups.push(Group::Decls(std::mem::take(current)));
            }
        };
    let flush_uses =
        |current: &mut Vec<&'a UseDecl>, groups: &mut Vec<Group<'a>>| {
            if !current.is_empty() {
                groups.push(Group::Uses(std::mem::take(current)));
            }
        };

    for item in items {
        match item {
            Item::Decl(d) => {
                flush_uses(&mut current_uses, &mut groups);
                current_decls.push(d);
            }
            Item::Use(u) => {
                flush_decls(&mut current_decls, &mut groups);
                current_uses.push(u);
            }
            Item::BlankLine => {
                flush_decls(&mut current_decls, &mut groups);
                flush_uses(&mut current_uses, &mut groups);
                groups.push(Group::Blank);
            }
            Item::FileDoc { text, .. } => {
                flush_decls(&mut current_decls, &mut groups);
                flush_uses(&mut current_uses, &mut groups);
                groups.push(Group::FileDoc(text.clone()));
            }
            Item::LineComment { text, .. } => {
                flush_decls(&mut current_decls, &mut groups);
                flush_uses(&mut current_uses, &mut groups);
                groups.push(Group::LineComment(text.clone()));
            }
        }
    }
    flush_decls(&mut current_decls, &mut groups);
    flush_uses(&mut current_uses, &mut groups);
    groups
}

fn format_use_group(uses: &[&UseDecl], out: &mut String) {
    // RFC 0003 §5: simplify single-brace, merge same-path imports, and sort.
    use std::collections::BTreeMap;
    let mut by_path: BTreeMap<String, std::collections::BTreeSet<String>> =
        BTreeMap::new();
    for u in uses {
        let path = u.path.join("::");
        let entry = by_path.entry(path).or_default();
        for it in &u.items {
            entry.insert(it.name.clone());
        }
    }
    for (path, names) in by_path {
        let names: Vec<String> = names.into_iter().collect();
        if names.len() == 1 {
            out.push_str(&format!("use {}::{}\n", path, names[0]));
        } else {
            out.push_str(&format!("use {}::{{{}}}\n", path, names.join(", ")));
        }
    }
}

fn format_decl_group(decls: &[&Decl], out: &mut String) {
    // Decide whether the group is alignable (all const decls). Mixed groups —
    // namespace, enum, type aliases — render normally without column alignment.
    let mut const_decls: Vec<&ConstDecl> = Vec::new();
    let mut all_const = true;
    for d in decls {
        if let DeclKind::Const(c) = &d.kind {
            // Only constants with no attributes participate in alignment.
            // Attributes shift the visual layout; aligning them gets messy.
            if d.attributes.is_empty() {
                const_decls.push(c);
            } else {
                all_const = false;
                break;
            }
        } else {
            all_const = false;
            break;
        }
    }

    if all_const && !const_decls.is_empty() {
        // Compute alignment widths.
        let type_strs: Vec<String> = const_decls.iter().map(|c| format_type(&c.type_expr)).collect();
        let name_strs: Vec<String> = const_decls.iter().map(|c| c.name.clone()).collect();
        let max_type = type_strs.iter().map(|s| s.len()).max().unwrap_or(0);
        let max_name = name_strs.iter().map(|s| s.len()).max().unwrap_or(0);

        for (i, decl) in decls.iter().enumerate() {
            // doc and attributes (no attributes since group requires none).
            if let Some(doc) = &decl.doc {
                emit_doc_block(doc, out, "");
            }
            let c = const_decls[i];
            let type_str = &type_strs[i];
            let name_str = &name_strs[i];
            // Column at which the value starts: padded type + space + padded name + " = ".
            let prefix_col = max_type + 1 + max_name + 3;
            let value_str = format_value_wrapped(&c.value, prefix_col, 0);
            out.push_str(&format!(
                "{:type_w$} {:name_w$} = {}\n",
                type_str,
                name_str,
                value_str,
                type_w = max_type,
                name_w = max_name,
            ));
        }
    } else {
        // Mixed group: render each decl on its own.
        for decl in decls {
            format_decl(decl, out, "");
        }
    }
}

fn emit_doc_block(doc: &DocBlock, out: &mut String, indent: &str) {
    for line in &doc.lines {
        if line.is_empty() {
            out.push_str(&format!("{}///\n", indent));
        } else {
            out.push_str(&format!("{}/// {}\n", indent, line));
        }
    }
}

fn emit_attributes(attrs: &[Attribute], out: &mut String, indent: &str) {
    for attr in attrs {
        if attr.args.is_empty() {
            out.push_str(&format!("{}@{}\n", indent, attr.name));
        } else {
            let args: Vec<String> = attr.args.iter().map(format_attr_arg).collect();
            out.push_str(&format!("{}@{}({})\n", indent, attr.name, args.join(", ")));
        }
    }
}

fn format_attr_arg(arg: &AttrArg) -> String {
    match arg {
        AttrArg::Ident(s) => s.clone(),
        AttrArg::Str(s) => format!("\"{}\"", escape_str(s)),
        AttrArg::Int(n) => n.to_string(),
        AttrArg::Bool(b) => b.to_string(),
    }
}

fn format_decl(decl: &Decl, out: &mut String, indent: &str) {
    if let Some(doc) = &decl.doc {
        emit_doc_block(doc, out, indent);
    }
    emit_attributes(&decl.attributes, out, indent);
    match &decl.kind {
        DeclKind::Namespace(ns) => {
            out.push_str(&format!("{}namespace {}\n", indent, ns.path.join("::")));
        }
        DeclKind::Const(c) => {
            let ts = format_type(&c.type_expr);
            // `<indent><type> <name> = ` is the prefix before the value.
            let prefix_col = indent.len() + ts.len() + 1 + c.name.len() + 3;
            let v = format_value_wrapped(&c.value, prefix_col, indent.len());
            out.push_str(&format!("{}{} {} = {}\n", indent, ts, c.name, v));
        }
        DeclKind::Enum(e) => format_enum(e, out, indent),
        DeclKind::TypeAlias(a) => {
            let target = format_type(&a.target);
            out.push_str(&format!("{}type {} = {}\n", indent, a.name, target));
        }
    }
}

fn format_enum(e: &EnumDecl, out: &mut String, indent: &str) {
    let backing = match &e.backing {
        Some(t) => format!(": {}", format_type(t)),
        None => String::new(),
    };
    out.push_str(&format!("{}enum {}{} {{\n", indent, e.name, backing));

    // Compute alignment for variant names if any have explicit values.
    let any_explicit = e.variants.iter().any(|v| v.value.is_some());
    let max_name = e
        .variants
        .iter()
        .map(|v| v.name.len())
        .max()
        .unwrap_or(0);

    let inner_indent = format!("{}{}", indent, INDENT);
    for v in &e.variants {
        if let Some(doc) = &v.doc {
            emit_doc_block(doc, out, &inner_indent);
        }
        if let Some(val) = &v.value {
            let val_str = format_value(val, false, inner_indent.len());
            if any_explicit {
                out.push_str(&format!(
                    "{}{:width$} = {},\n",
                    inner_indent,
                    v.name,
                    val_str,
                    width = max_name,
                ));
            } else {
                out.push_str(&format!("{}{} = {},\n", inner_indent, v.name, val_str));
            }
        } else if any_explicit {
            // No explicit value but other variants have one — leave the `= ...` part off.
            out.push_str(&format!("{}{},\n", inner_indent, v.name));
        } else {
            out.push_str(&format!("{}{},\n", inner_indent, v.name));
        }
    }
    out.push_str(&format!("{}}}\n", indent));
}

fn format_type(t: &TypeExpr) -> String {
    match &t.kind {
        TypeExprKind::Named { path } => path.join("::"),
        TypeExprKind::Array(inner) | TypeExprKind::ArrayGeneric(inner) => {
            format!("{}[]", format_type(inner))
        }
        TypeExprKind::Optional(inner) | TypeExprKind::OptionalGeneric(inner) => {
            format!("{}?", format_type(inner))
        }
        TypeExprKind::Map { key, value } => {
            format!("map<{}, {}>", format_type(key), format_type(value))
        }
        TypeExprKind::Tuple(elems) => {
            let parts: Vec<String> = elems.iter().map(format_type).collect();
            format!("tuple<{}>", parts.join(", "))
        }
        TypeExprKind::FixedArrayGeneric { element, length } => {
            format!("array<{}, {}>", format_type(element), length)
        }
    }
}

/// Render `v` either single-line or multi-line based on whether it would fit
/// within `COLUMN_BUDGET` when prefixed by `prefix_col` characters on the
/// opening line. Multi-line output uses `outer_indent` as the indent of the
/// closing delimiter; inner items get `outer_indent + 4`.
///
/// Falls back to single-line for value kinds that can't be sensibly wrapped
/// (scalars, identifiers, negation of scalars).
fn format_value_wrapped(v: &ValueExpr, prefix_col: usize, outer_indent: usize) -> String {
    // RFC 0003 §4 magic trailing comma: a literal whose last element had a
    // trailing comma in source stays multi-line regardless of column budget.
    let force_multiline = matches!(
        &v.kind,
        ValueExprKind::Array { trailing_comma: true, .. }
            | ValueExprKind::Tuple { trailing_comma: true, .. }
            | ValueExprKind::Map { trailing_comma: true, .. },
    );
    if !force_multiline {
        let single = format_value(v, false, outer_indent);
        if prefix_col + single.len() <= COLUMN_BUDGET {
            return single;
        }
    }
    match &v.kind {
        ValueExprKind::Array { items, .. } | ValueExprKind::Tuple { items, .. } => {
            wrap_value_collection(items, "[", "]", outer_indent)
        }
        ValueExprKind::Map { entries, .. } => wrap_value_map(entries, outer_indent),
        _ => format_value(v, false, outer_indent),
    }
}

fn wrap_value_collection(
    items: &[ValueExpr],
    open: &str,
    close: &str,
    outer_indent: usize,
) -> String {
    if items.is_empty() {
        return format!("{}{}", open, close);
    }
    let inner = outer_indent + 4;
    let outer_pad = " ".repeat(outer_indent);
    let inner_pad = " ".repeat(inner);
    let mut s = String::new();
    s.push_str(open);
    s.push('\n');
    for item in items {
        s.push_str(&inner_pad);
        s.push_str(&format_value_wrapped(item, inner, inner));
        s.push_str(",\n");
    }
    s.push_str(&outer_pad);
    s.push_str(close);
    s
}

fn wrap_value_map(entries: &[(MapKey, ValueExpr)], outer_indent: usize) -> String {
    if entries.is_empty() {
        return "{}".into();
    }
    let inner = outer_indent + 4;
    let outer_pad = " ".repeat(outer_indent);
    let inner_pad = " ".repeat(inner);
    let mut s = String::from("{\n");
    for (k, v) in entries {
        let key_str = format_map_key(k);
        let prefix_col = inner + key_str.len() + 2; // "key: "
        let val_str = format_value_wrapped(v, prefix_col, inner);
        s.push_str(&inner_pad);
        s.push_str(&format!("{}: {},\n", key_str, val_str));
    }
    s.push_str(&outer_pad);
    s.push('}');
    s
}

fn format_value(v: &ValueExpr, _trailing_comma: bool, indent_level: usize) -> String {
    match &v.kind {
        ValueExprKind::Int { value, suffix } => match suffix {
            Some(s) => format!("{}{}", value, s),
            None => format!("{}", value),
        },
        ValueExprKind::Float { value, suffix } => match suffix {
            Some(s) => format!("{}{}", value, s),
            None => {
                let s = value.to_string();
                if s.contains('.') || s.contains('e') {
                    s
                } else {
                    format!("{}.0", s)
                }
            }
        },
        ValueExprKind::Bool(b) => b.to_string(),
        ValueExprKind::Str(s) => format!("\"{}\"", escape_str(s)),
        ValueExprKind::None_ => "none".to_string(),
        ValueExprKind::Path { path } => path.join("::"),
        ValueExprKind::Neg(inner) => format!("-{}", format_value(inner, false, indent_level)),
        ValueExprKind::Array { items, .. } => {
            let parts: Vec<String> = items.iter().map(|i| format_value(i, false, indent_level)).collect();
            format!("[{}]", parts.join(", "))
        }
        ValueExprKind::Tuple { items, .. } => {
            // RFC 0003 §1: tuple values are written `[...]`. The Tuple variant
            // is no longer produced by the parser, but we keep this arm for
            // compatibility if a caller hand-builds the AST.
            let parts: Vec<String> = items.iter().map(|i| format_value(i, false, indent_level)).collect();
            format!("[{}]", parts.join(", "))
        }
        ValueExprKind::Map { entries, .. } => {
            let parts: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{}: {}", format_map_key(k), format_value(v, false, indent_level)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn format_map_key(k: &MapKey) -> String {
    match &k.kind {
        MapKeyKind::Str(s) => format!("\"{}\"", escape_str(s)),
        MapKeyKind::Ident(s) => s.clone(),
        MapKeyKind::Int(n) => n.to_string(),
    }
}

fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

// silences unused import in some builds
fn _ensure_lexer_used(_t: lexer::Token) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(input: &str) -> String {
        format_source(input).unwrap()
    }

    #[test]
    fn aligns_consecutive_consts() {
        let input = "duration TIMEOUT = 30s\nu32 MAX_USERS = 8\nbytes UPLOAD = 100MiB\n";
        let out = fmt(input);
        assert_eq!(
            out,
            "duration TIMEOUT   = 30s\nu32      MAX_USERS = 8\nbytes    UPLOAD    = 100MiB\n"
        );
    }

    #[test]
    fn blank_line_breaks_alignment() {
        let input = "u32 SHORT = 1\n\nduration LONGER_NAME = 30s\n";
        let out = fmt(input);
        // Both groups have a single declaration each, so alignment is per-group.
        assert!(out.contains("u32 SHORT = 1\n"));
        assert!(out.contains("duration LONGER_NAME = 30s\n"));
        assert!(out.contains("\n\n"));
    }

    #[test]
    fn doc_attaches_and_does_not_break_group() {
        let input = "/// Doc.\nu32 A = 1\nu32 BB = 2\n";
        let out = fmt(input);
        assert!(out.contains("/// Doc.\nu32 A  = 1\n"));
    }

    #[test]
    fn formats_enum() {
        let input = "enum Status { Pending, Active, Done }\n";
        let out = fmt(input);
        assert!(out.contains("enum Status {"));
        assert!(out.contains("    Pending,"));
    }

    #[test]
    fn formats_int_enum_with_alignment() {
        let input = "enum Lvl: u8 { Debug = 0, Info = 1, Warn = 2 }\n";
        let out = fmt(input);
        assert!(out.contains("Debug = 0,"));
        assert!(out.contains("Info  = 1,"));
    }

    #[test]
    fn formats_type_alias() {
        let input = "type Port = u16\n";
        let out = fmt(input);
        assert_eq!(out, "type Port = u16\n");
    }

    #[test]
    fn wraps_long_array_value() {
        // RFC 0003 §4: lines past column 100 wrap at the shallowest fitting delimiter.
        // 16 entries × ~10 chars each = ~160 columns, well over budget.
        let input = "type Big = array<u32>\nBig DATA = [1000000, 1000000, 1000000, 1000000, 1000000, 1000000, 1000000, 1000000, 1000000, 1000000, 1000000, 1000000]\n";
        let out = fmt(input);
        // Each item should be on its own line.
        let n_lines = out.matches('\n').count();
        assert!(n_lines >= 13, "expected wrapped output, got: {}", out);
        // Closing bracket on its own line.
        assert!(out.contains("\n]\n"), "expected closing bracket on own line, got: {}", out);
        // Trailing comma after last item.
        assert!(out.contains("1000000,\n]\n"), "expected trailing comma, got: {}", out);
    }

    #[test]
    fn short_array_stays_single_line() {
        let input = "type Small = array<u32>\nSmall DATA = [1, 2, 3]\n";
        let out = fmt(input);
        assert!(out.contains("[1, 2, 3]"), "{}", out);
    }

    #[test]
    fn use_block_sorts_and_merges() {
        // RFC 0003 §5: simplify single-brace, merge same-path, sort.
        let input = "use a::b::{Z}\nuse a::b::Y\nuse a::b::{X, A}\n";
        let out = fmt(input);
        assert_eq!(out, "use a::b::{A, X, Y, Z}\n");
    }

    #[test]
    fn trailing_comma_keeps_array_multiline() {
        // RFC 0003 §4 magic trailing comma: even though this fits on one line,
        // the trailing comma signals the user wants multi-line.
        let input = "type V3 = array<u32, 3>\nV3 X = [1, 2, 3,]\n";
        let out = fmt(input);
        assert!(
            out.contains("[\n    1,\n    2,\n    3,\n]"),
            "expected multi-line, got: {}",
            out
        );
    }

    #[test]
    fn no_trailing_comma_stays_compact_when_fits() {
        let input = "type V3 = array<u32, 3>\nV3 X = [1, 2, 3]\n";
        let out = fmt(input);
        assert!(out.contains("[1, 2, 3]"), "{}", out);
    }

    #[test]
    fn trailing_comma_keeps_matrix_multiline() {
        // The motivating case: a 3x3 matrix that would fit on one line but
        // reads better as a multi-line block.
        let input = "type Mat = array<array<f64, 3>, 3>\nMat I = [\n    [1, 0, 0],\n    [0, 1, 0],\n    [0, 0, 1],\n]\n";
        let out = fmt(input);
        assert!(out.contains("[\n    [1, 0, 0],"), "expected multi-line outer, got: {}", out);
        assert!(out.contains("[0, 0, 1],\n]"), "expected trailing comma on last row, got: {}", out);
    }

    #[test]
    fn use_single_item_collapses() {
        let input = "use a::b::{Foo}\n";
        let out = fmt(input);
        assert_eq!(out, "use a::b::Foo\n");
    }
}
