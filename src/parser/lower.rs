//! Lower the AST to the IR with cross-file type resolution.
//!
//! Two-pass design:
//! 1. Collect every top-level enum and type alias from every file, indexed by
//!    `(namespace, name)`. Detect duplicates here.
//! 2. Lower each file: resolve type-name references against the index, validate
//!    constant values against types, and emit IR.

use super::ast::*;
use super::lexer::Span;
use crate::diagnostics::{Diagnostic, Diagnostics};
use crate::ir::{
    Constant, EnumDef, EnumVariant, Module, SourceLocation, TypeAliasDef,
};
use crate::types::{
    is_screaming_snake_case, parse_duration, validate_regex, validate_url, Type, Value,
};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct ResolvedProject {
    pub modules: Vec<Module>,
    pub enums: Vec<EnumDef>,
    pub aliases: Vec<TypeAliasDef>,
    pub diagnostics: Diagnostics,
}

#[derive(Debug, Clone)]
struct EnumIndexEntry {
    #[allow(dead_code)]
    backing: EnumBacking,
    variants: Vec<String>, // variant names in declaration order
    #[allow(dead_code)]
    namespace: String,
    /// `true` if integer-backed; `false` if string-tagged.
    int_backed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EnumBacking {
    /// Untagged: variants identified by name; serialized as their PascalCase string.
    Untagged,
    /// Integer-backed (i32 backing assumed for now).
    Integer,
}

#[derive(Debug, Clone)]
struct AliasIndexEntry {
    target: Type,
    #[allow(dead_code)]
    namespace: String,
}

pub struct ParsedFile {
    pub path: std::path::PathBuf,
    pub default_namespace: String,
    pub ast: File,
    pub source_text: String,
}

pub fn lower(files: Vec<ParsedFile>) -> ResolvedProject {
    let mut diagnostics = Diagnostics::new();
    let mut effective: Vec<(ParsedFile, String)> = Vec::new();

    // Pass 0: determine each file's effective namespace (override or default)
    for file in files {
        let ns = effective_namespace(&file, &mut diagnostics);
        effective.push((file, ns));
    }

    // Pass 1: build indices of enums and aliases. Detect duplicates.
    let mut enum_index: HashMap<(String, String), (EnumIndexEntry, SourceLocation)> = HashMap::new();
    let mut alias_index: HashMap<(String, String), (AliasIndexEntry, SourceLocation)> =
        HashMap::new();
    let mut const_names: HashMap<(String, String), SourceLocation> = HashMap::new();

    let mut enum_decls: Vec<EnumDef> = Vec::new();
    let mut alias_decls: Vec<TypeAliasDef> = Vec::new();

    for (file, namespace) in &effective {
        for item in &file.ast.items {
            let decl = match item {
                Item::Decl(d) => d,
                _ => continue,
            };
            match &decl.kind {
                DeclKind::Enum(e) => {
                    let key = (namespace.clone(), e.name.clone());
                    let source = source_loc(&file.path, decl.span);
                    if !is_pascal_case(&e.name) {
                        diagnostics.add(
                            Diagnostic::error(
                                &source,
                                "naming-convention",
                                format!("enum name `{}` must be PascalCase", e.name),
                            )
                            .with_targets(vec![]),
                        );
                    }
                    if let Some(existing) = enum_index.get(&key) {
                        diagnostics.add(Diagnostic::error(
                            &source,
                            "duplicate-name",
                            format!(
                                "enum `{}` is already declared in namespace `{}` (first declared at {}:{})",
                                e.name, namespace, existing.1.file, existing.1.line
                            ),
                        ));
                        continue;
                    }
                    if let Some(existing) = alias_index.get(&key) {
                        diagnostics.add(Diagnostic::error(
                            &source,
                            "duplicate-name",
                            format!(
                                "enum `{}` collides with type alias of the same name (declared at {}:{})",
                                e.name, existing.1.file, existing.1.line
                            ),
                        ));
                        continue;
                    }
                    // Resolve backing as both: (a) the boolean "is this int-backed?",
                    // and (b) the bare-name range for bounds-checking variant values
                    // (RFC 0004 §3) — `enum Foo: u8` checks each variant against
                    // 0..=255, not against u32's range.
                    let (int_backed, backing_range, backing_name): (bool, Option<(i128, i128)>, Option<String>) =
                        match &e.backing {
                            Some(t) => {
                                let bare_name = match &t.kind {
                                    TypeExprKind::Named { path: segs } if segs.len() == 1 => {
                                        Some(segs[0].clone())
                                    }
                                    _ => None,
                                };
                                let range = bare_name.as_deref().and_then(int_range_from_name);
                                if range.is_none() {
                                    diagnostics.add(Diagnostic::error(
                                        &source_loc(&file.path, t.span),
                                        "invalid-enum-backing",
                                        "enum backing type must be an integer (i8/i16/i32/i64/u8/u16/u32/u64)".to_string(),
                                    ));
                                    (false, None, None)
                                } else {
                                    (true, range, bare_name)
                                }
                            }
                            None => (false, None, None),
                        };

                    let mut variants = Vec::new();
                    let mut variant_names: Vec<String> = Vec::new();
                    let mut next_int: i128 = 0;
                    let mut seen_variants: HashMap<String, ()> = HashMap::new();
                    for v in &e.variants {
                        if !is_pascal_case(&v.name) {
                            diagnostics.add(Diagnostic::error(
                                &source_loc(&file.path, v.name_span),
                                "naming-convention",
                                format!("enum variant `{}` must be PascalCase", v.name),
                            ));
                        }
                        if seen_variants.contains_key(&v.name) {
                            diagnostics.add(Diagnostic::error(
                                &source_loc(&file.path, v.name_span),
                                "duplicate-name",
                                format!("duplicate enum variant `{}`", v.name),
                            ));
                            continue;
                        }
                        seen_variants.insert(v.name.clone(), ());

                        let value = if int_backed {
                            let (n, value_span) = match &v.value {
                                Some(ve) => match constant_int(&ve.kind) {
                                    Some(n) => {
                                        next_int = n + 1;
                                        (n, ve.span)
                                    }
                                    None => {
                                        diagnostics.add(Diagnostic::error(
                                            &source_loc(&file.path, ve.span),
                                            "invalid-enum-variant",
                                            "integer-backed enum variants need an integer literal value".to_string(),
                                        ));
                                        next_int += 1;
                                        (next_int - 1, ve.span)
                                    }
                                },
                                None => {
                                    let n = next_int;
                                    next_int += 1;
                                    (n, v.name_span)
                                }
                            };
                            // RFC 0004 §3: bounds-check the variant value
                            // against the backing type's range.
                            if let (Some((min, max)), Some(name)) =
                                (backing_range, backing_name.as_deref())
                            {
                                if n < min || n > max {
                                    diagnostics.add(Diagnostic::error(
                                        &source_loc(&file.path, value_span),
                                        "out-of-range",
                                        format!(
                                            "value {} does not fit in {} (range: {}..={})",
                                            n, name, min, max,
                                        ),
                                    ));
                                }
                            }
                            Value::Integer(n)
                        } else if let Some(ve) = &v.value {
                            // Untagged enum with explicit value: treat string as override of name.
                            match &ve.kind {
                                ValueExprKind::Str(s) => Value::String(s.clone()),
                                _ => {
                                    diagnostics.add(Diagnostic::error(
                                        &source_loc(&file.path, ve.span),
                                        "invalid-enum-variant",
                                        "untyped enum variants accept a string override or no value".to_string(),
                                    ));
                                    Value::String(v.name.clone())
                                }
                            }
                        } else {
                            Value::String(v.name.clone())
                        };
                        variants.push(EnumVariant {
                            name: v.name.clone(),
                            value,
                        });
                        variant_names.push(v.name.clone());
                    }

                    let entry = EnumIndexEntry {
                        backing: if int_backed {
                            EnumBacking::Integer
                        } else {
                            EnumBacking::Untagged
                        },
                        variants: variant_names,
                        namespace: namespace.clone(),
                        int_backed,
                    };
                    enum_index.insert(key.clone(), (entry, source.clone()));
                    let doc = decl.doc.as_ref().map(|d| d.joined());
                    enum_decls.push(EnumDef {
                        name: e.name.clone(),
                        namespace: namespace.clone(),
                        doc,
                        variants,
                        backing_type: if int_backed {
                            "integer".into()
                        } else {
                            "string".into()
                        },
                        source,
                    });
                }
                DeclKind::TypeAlias(a) => {
                    let key = (namespace.clone(), a.name.clone());
                    let source = source_loc(&file.path, decl.span);
                    if !is_pascal_case(&a.name) {
                        diagnostics.add(Diagnostic::error(
                            &source,
                            "naming-convention",
                            format!("type alias `{}` must be PascalCase", a.name),
                        ));
                    }
                    if let Some(existing) = enum_index.get(&key) {
                        diagnostics.add(Diagnostic::error(
                            &source,
                            "duplicate-name",
                            format!(
                                "type alias `{}` collides with enum of the same name (declared at {}:{})",
                                a.name, existing.1.file, existing.1.line
                            ),
                        ));
                        continue;
                    }
                    if let Some(existing) = alias_index.get(&key) {
                        diagnostics.add(Diagnostic::error(
                            &source,
                            "duplicate-name",
                            format!(
                                "type alias `{}` is already declared in namespace `{}` (first declared at {}:{})",
                                a.name, namespace, existing.1.file, existing.1.line
                            ),
                        ));
                        continue;
                    }
                    // Defer alias target resolution to pass 2 — at that point all
                    // enums and aliases are indexed and we can resolve forward refs.
                    alias_index.insert(
                        key.clone(),
                        (
                            AliasIndexEntry {
                                target: Type::String, // placeholder, replaced in pass 2
                                namespace: namespace.clone(),
                            },
                            source.clone(),
                        ),
                    );
                    let doc = decl.doc.as_ref().map(|d| d.joined());
                    alias_decls.push(TypeAliasDef {
                        name: a.name.clone(),
                        namespace: namespace.clone(),
                        doc,
                        target: Type::String, // placeholder
                        source,
                    });
                }
                DeclKind::Const(c) => {
                    let key = (namespace.clone(), c.name.clone());
                    let source = source_loc(&file.path, decl.span);
                    if let Some(existing) = const_names.get(&key) {
                        diagnostics.add(Diagnostic::error(
                            &source,
                            "duplicate-name",
                            format!(
                                "constant `{}` is already declared in namespace `{}` (first declared at {}:{})",
                                c.name, namespace, existing.file, existing.line
                            ),
                        ));
                    } else {
                        const_names.insert(key, source);
                    }
                }
                DeclKind::Namespace(_) => {}
            }
        }
    }

    // Pass 1.5: collect each file's `use` imports. Validate that each imported
    // name resolves; reject collisions with same-namespace declarations and
    // with sibling imports.
    //
    // RFC 0003 §3 — `unused-import` is a warning we'll add later; for now we
    // only emit hard errors here.
    let mut file_imports: HashMap<std::path::PathBuf, HashMap<String, String>> = HashMap::new();
    for (file, namespace) in &effective {
        let mut imports: HashMap<String, String> = HashMap::new();
        for item in &file.ast.items {
            let u = match item {
                Item::Use(u) => u,
                _ => continue,
            };
            let import_ns = u.path.join("::");
            for it in &u.items {
                let key = (import_ns.clone(), it.name.clone());
                let exists = enum_index.contains_key(&key) || alias_index.contains_key(&key);
                if !exists {
                    diagnostics.add(Diagnostic::error(
                        &source_loc(&file.path, it.span),
                        "unresolved-import",
                        format!("`{}::{}` does not exist", import_ns, it.name),
                    ));
                    continue;
                }
                let local_key = (namespace.clone(), it.name.clone());
                if enum_index.contains_key(&local_key)
                    || alias_index.contains_key(&local_key)
                {
                    diagnostics.add(Diagnostic::error(
                        &source_loc(&file.path, it.span),
                        "import-collision",
                        format!(
                            "imported name `{}` collides with a declaration in the current namespace",
                            it.name
                        ),
                    ));
                    continue;
                }
                if let Some(other_ns) = imports.get(&it.name) {
                    diagnostics.add(Diagnostic::error(
                        &source_loc(&file.path, it.span),
                        "import-collision",
                        format!("`{}` is already imported from `{}`", it.name, other_ns),
                    ));
                    continue;
                }
                imports.insert(it.name.clone(), import_ns.clone());
            }
        }
        file_imports.insert(file.path.clone(), imports);
    }
    let empty_imports: HashMap<String, String> = HashMap::new();

    // Pass 2: resolve alias targets now that the alias/enum indices are populated.
    let mut alias_targets: HashMap<(String, String), Type> = HashMap::new();
    for (file, namespace) in &effective {
        let imports = file_imports.get(&file.path).unwrap_or(&empty_imports);
        for item in &file.ast.items {
            let decl = match item {
                Item::Decl(d) => d,
                _ => continue,
            };
            if let DeclKind::TypeAlias(a) = &decl.kind {
                let resolved = resolve_type_expr(
                    &a.target,
                    namespace,
                    imports,
                    &enum_index,
                    &alias_index,
                    &file.path,
                    &mut diagnostics,
                );
                let resolved = resolved.unwrap_or(Type::String);
                alias_targets.insert((namespace.clone(), a.name.clone()), resolved);
            }
        }
    }

    // Patch alias_decls and alias_index with resolved targets.
    for alias_def in alias_decls.iter_mut() {
        if let Some(target) = alias_targets.get(&(alias_def.namespace.clone(), alias_def.name.clone())) {
            alias_def.target = target.clone();
        }
    }
    for ((ns, name), entry) in alias_index.iter_mut() {
        if let Some(t) = alias_targets.get(&(ns.clone(), name.clone())) {
            entry.0.target = t.clone();
        }
    }

    // Track which aliases are @inline (suppressed from emission)
    let mut inline_aliases: HashMap<(String, String), bool> = HashMap::new();
    let mut alias_only_targets: HashMap<(String, String), Type> = HashMap::new();
    for (file, namespace) in &effective {
        for item in &file.ast.items {
            let decl = match item {
                Item::Decl(d) => d,
                _ => continue,
            };
            if let DeclKind::TypeAlias(a) = &decl.kind {
                let inline = decl
                    .attributes
                    .iter()
                    .any(|attr| attr.name == "inline");
                inline_aliases.insert((namespace.clone(), a.name.clone()), inline);
                if let Some(t) = alias_targets.get(&(namespace.clone(), a.name.clone())) {
                    alias_only_targets.insert((namespace.clone(), a.name.clone()), t.clone());
                }
            }
        }
    }

    // Drop @inline aliases from emission.
    alias_decls.retain(|a| !*inline_aliases.get(&(a.namespace.clone(), a.name.clone())).unwrap_or(&false));

    // Pass 3: lower constants per file.
    let mut modules: Vec<Module> = Vec::new();
    let mut module_by_ns: HashMap<String, Module> = HashMap::new();

    for (file, namespace) in &effective {
        let imports = file_imports.get(&file.path).unwrap_or(&empty_imports);
        let mut module_doc: Option<String> = None;
        for item in &file.ast.items {
            if let Item::FileDoc { text, .. } = item {
                let line = text.clone();
                module_doc = Some(match module_doc {
                    Some(prev) => format!("{}\n{}", prev, line),
                    None => line,
                });
            }
        }

        for item in &file.ast.items {
            let decl = match item {
                Item::Decl(d) => d,
                _ => continue,
            };
            if let DeclKind::Const(c) = &decl.kind {
                let source = source_loc(&file.path, decl.span);
                if !is_screaming_snake_case(&c.name) {
                    diagnostics.add(Diagnostic::error(
                        &source_loc(&file.path, c.name_span),
                        "naming-convention",
                        format!("constant `{}` must be SCREAMING_SNAKE_CASE", c.name),
                    ));
                }
                let resolved_type = match resolve_type_expr(
                    &c.type_expr,
                    namespace,
                    imports,
                    &enum_index,
                    &alias_index,
                    &file.path,
                    &mut diagnostics,
                ) {
                    Some(t) => t,
                    None => continue,
                };

                // Expand inline aliases on use.
                let typ = expand_inline_aliases(&resolved_type, &inline_aliases, &alias_only_targets);

                // For value normalization, fully resolve aliases to their underlying types.
                let normalize_typ = fully_resolve_aliases(&typ, &alias_only_targets);
                let value = match normalize_value(
                    &normalize_typ,
                    &c.value,
                    &enum_index,
                    namespace,
                    &file.path,
                    &mut diagnostics,
                ) {
                    Some(v) => v,
                    None => continue,
                };

                let doc = decl.doc.as_ref().map(|d| d.joined());
                let constant = Constant {
                    name: c.name.clone(),
                    doc,
                    typ,
                    value,
                    source,
                };
                module_by_ns
                    .entry(namespace.clone())
                    .or_insert_with(|| Module {
                        namespace: namespace.clone(),
                        source_file: file.path.display().to_string(),
                        doc: module_doc.clone(),
                        constants: Vec::new(),
                    })
                    .constants
                    .push(constant);
            }
        }
    }

    for (_, m) in module_by_ns {
        modules.push(m);
    }

    ResolvedProject {
        modules,
        enums: enum_decls,
        aliases: alias_decls,
        diagnostics,
    }
}

fn effective_namespace(file: &ParsedFile, diagnostics: &mut Diagnostics) -> String {
    let mut found: Option<String> = None;
    for item in &file.ast.items {
        if let Item::Decl(d) = item {
            if let DeclKind::Namespace(ns) = &d.kind {
                if found.is_some() {
                    diagnostics.add(Diagnostic::error(
                        &source_loc(&file.path, ns.path_span),
                        "duplicate-namespace",
                        "only one `namespace` declaration is allowed per file".to_string(),
                    ));
                } else {
                    for seg in &ns.path {
                        if !is_lower_snake(seg) {
                            diagnostics.add(Diagnostic::error(
                                &source_loc(&file.path, ns.path_span),
                                "naming-convention",
                                format!(
                                    "namespace segment `{}` must be lower_snake_case",
                                    seg
                                ),
                            ));
                        }
                    }
                    found = Some(ns.path.join("::"));
                }
            }
        }
    }
    found.unwrap_or_else(|| file.default_namespace.clone())
}

fn source_loc(path: &std::path::Path, span: Span) -> SourceLocation {
    SourceLocation {
        file: path.display().to_string(),
        line: span.line,
        column: span.column,
        length: Some(span.len()),
    }
}

fn is_pascal_case(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    for c in chars {
        if !c.is_ascii_alphanumeric() {
            return false;
        }
    }
    !s.is_empty()
}

fn is_lower_snake(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !s.starts_with('_')
        && !s.ends_with('_')
        && !s.contains("__")
}

fn constant_int(kind: &ValueExprKind) -> Option<i128> {
    match kind {
        ValueExprKind::Int { value, suffix: None } => Some(*value),
        ValueExprKind::Neg(inner) => match &inner.kind {
            ValueExprKind::Int { value, suffix: None } => Some(-*value),
            _ => None,
        },
        _ => None,
    }
}

/// Range of valid values for a primate primitive integer name. Used for
/// bounds-checking literals (RFC 0004 §3) — including narrow types valid
/// only as enum backings.
fn int_range_from_name(name: &str) -> Option<(i128, i128)> {
    Some(match name {
        "i8" => (i8::MIN as i128, i8::MAX as i128),
        "i16" => (i16::MIN as i128, i16::MAX as i128),
        "i32" => (i32::MIN as i128, i32::MAX as i128),
        "i64" => (i64::MIN as i128, i64::MAX as i128),
        "u8" => (0, u8::MAX as i128),
        "u16" => (0, u16::MAX as i128),
        "u32" => (0, u32::MAX as i128),
        "u64" => (0, u64::MAX as i128),
        _ => return None,
    })
}

fn int_range_for_type(ty: &Type) -> Option<(i128, i128, &'static str)> {
    Some(match ty {
        Type::I32 => (i32::MIN as i128, i32::MAX as i128, "i32"),
        Type::I64 => (i64::MIN as i128, i64::MAX as i128, "i64"),
        Type::U32 => (0, u32::MAX as i128, "u32"),
        Type::U64 => (0, u64::MAX as i128, "u64"),
        _ => return None,
    })
}

fn primitive_from_name(s: &str) -> Option<Type> {
    Some(match s {
        "i32" => Type::I32,
        "i64" => Type::I64,
        "u32" => Type::U32,
        "u64" => Type::U64,
        "f32" => Type::F32,
        "f64" => Type::F64,
        "bool" => Type::Bool,
        "string" => Type::String,
        "duration" => Type::Duration,
        "regex" => Type::Regex,
        "url" => Type::Url,
        // Narrow integer types are accepted in enum-backing position only;
        // in any other position they must be widened by the user (i32/u32 etc.).
        // This is a temporary limitation — the IR doesn't model narrow ints yet.
        _ => return None,
    })
}

/// Like `primitive_from_name`, but additionally accepts the narrow integer
/// types valid for enum backing.
fn resolve_type_expr(
    expr: &TypeExpr,
    current_namespace: &str,
    imports: &HashMap<String, String>,
    enums: &HashMap<(String, String), (EnumIndexEntry, SourceLocation)>,
    aliases: &HashMap<(String, String), (AliasIndexEntry, SourceLocation)>,
    path: &std::path::Path,
    diags: &mut Diagnostics,
) -> Option<Type> {
    match &expr.kind {
        TypeExprKind::Named { path: segs } => {
            if segs.len() == 1 {
                let name = &segs[0];
                if let Some(t) = primitive_from_name(name) {
                    return Some(t);
                }
                // Look in current namespace for enum/alias
                if enums.contains_key(&(current_namespace.to_string(), name.clone())) {
                    return Some(Type::Enum {
                        name: name.clone(),
                        namespace: current_namespace.to_string(),
                    });
                }
                if aliases.contains_key(&(current_namespace.to_string(), name.clone())) {
                    return Some(Type::Alias {
                        name: name.clone(),
                        namespace: current_namespace.to_string(),
                    });
                }
                // RFC 0003 §3: a name brought into scope by a `use` statement.
                if let Some(import_ns) = imports.get(name) {
                    let key = (import_ns.clone(), name.clone());
                    if enums.contains_key(&key) {
                        return Some(Type::Enum {
                            name: name.clone(),
                            namespace: import_ns.clone(),
                        });
                    }
                    if aliases.contains_key(&key) {
                        return Some(Type::Alias {
                            name: name.clone(),
                            namespace: import_ns.clone(),
                        });
                    }
                }
                diags.add(Diagnostic::error(
                    &source_loc(path, expr.span),
                    "unknown-type",
                    format!("unknown type `{}`", name),
                ));
                None
            } else {
                let (ns_segs, name) = segs.split_at(segs.len() - 1);
                let ns = ns_segs.join("::");
                let name = &name[0];
                if enums.contains_key(&(ns.clone(), name.clone())) {
                    return Some(Type::Enum {
                        name: name.clone(),
                        namespace: ns.clone(),
                    });
                }
                if aliases.contains_key(&(ns.clone(), name.clone())) {
                    return Some(Type::Alias {
                        name: name.clone(),
                        namespace: ns.clone(),
                    });
                }
                diags.add(Diagnostic::error(
                    &source_loc(path, expr.span),
                    "unknown-type",
                    format!("unknown type `{}::{}`", ns, name),
                ));
                None
            }
        }
        TypeExprKind::Array(inner) | TypeExprKind::ArrayGeneric(inner) => {
            let inner_t = resolve_type_expr(inner, current_namespace, imports, enums, aliases, path, diags)?;
            Some(Type::Array {
                element: Box::new(inner_t),
            })
        }
        TypeExprKind::Optional(inner) | TypeExprKind::OptionalGeneric(inner) => {
            let inner_t = resolve_type_expr(inner, current_namespace, imports, enums, aliases, path, diags)?;
            Some(Type::Optional {
                inner: Box::new(inner_t),
            })
        }
        TypeExprKind::Map { key, value } => {
            let k = resolve_type_expr(key, current_namespace, imports, enums, aliases, path, diags)?;
            let v = resolve_type_expr(value, current_namespace, imports, enums, aliases, path, diags)?;
            Some(Type::Map {
                key: Box::new(k),
                value: Box::new(v),
            })
        }
        TypeExprKind::Tuple(elems) => {
            let mut ts = Vec::new();
            for e in elems {
                ts.push(resolve_type_expr(e, current_namespace, imports, enums, aliases, path, diags)?);
            }
            Some(Type::Tuple { elements: ts })
        }
        TypeExprKind::FixedArrayGeneric { element, length } => {
            let inner_t = resolve_type_expr(
                element,
                current_namespace,
                imports,
                enums,
                aliases,
                path,
                diags,
            )?;
            Some(Type::FixedArray {
                element: Box::new(inner_t),
                length: *length,
            })
        }
    }
}

/// Fully resolve all `Type::Alias` occurrences in `typ` to their underlying
/// targets. Used when normalizing values, where the value carries the
/// representation of the underlying type.
fn fully_resolve_aliases(
    typ: &Type,
    targets: &HashMap<(String, String), Type>,
) -> Type {
    match typ {
        Type::Alias { name, .. } => {
            let candidate = targets.iter().find_map(|((_, n), v)| if n == name { Some(v) } else { None });
            match candidate {
                Some(t) => fully_resolve_aliases(t, targets),
                None => typ.clone(),
            }
        }
        Type::Array { element } => Type::Array {
            element: Box::new(fully_resolve_aliases(element, targets)),
        },
        Type::FixedArray { element, length } => Type::FixedArray {
            element: Box::new(fully_resolve_aliases(element, targets)),
            length: *length,
        },
        Type::Optional { inner } => Type::Optional {
            inner: Box::new(fully_resolve_aliases(inner, targets)),
        },
        Type::Map { key, value } => Type::Map {
            key: Box::new(fully_resolve_aliases(key, targets)),
            value: Box::new(fully_resolve_aliases(value, targets)),
        },
        Type::Tuple { elements } => Type::Tuple {
            elements: elements.iter().map(|e| fully_resolve_aliases(e, targets)).collect(),
        },
        other => other.clone(),
    }
}

fn expand_inline_aliases(
    typ: &Type,
    inline: &HashMap<(String, String), bool>,
    targets: &HashMap<(String, String), Type>,
) -> Type {
    match typ {
        Type::Alias { name, .. } => {
            // Look up in any namespace; alias names should already be unique enough
            // — when ambiguous, fall back to leaving the alias as-is.
            for ((_ns, n), is_inline) in inline {
                if n == name && *is_inline {
                    if let Some(t) = targets.iter().find_map(|((_, k), v)| if k == name { Some(v) } else { None }) {
                        return expand_inline_aliases(t, inline, targets);
                    }
                }
            }
            typ.clone()
        }
        Type::Array { element } => Type::Array {
            element: Box::new(expand_inline_aliases(element, inline, targets)),
        },
        Type::FixedArray { element, length } => Type::FixedArray {
            element: Box::new(expand_inline_aliases(element, inline, targets)),
            length: *length,
        },
        Type::Optional { inner } => Type::Optional {
            inner: Box::new(expand_inline_aliases(inner, inline, targets)),
        },
        Type::Map { key, value } => Type::Map {
            key: Box::new(expand_inline_aliases(key, inline, targets)),
            value: Box::new(expand_inline_aliases(value, inline, targets)),
        },
        Type::Tuple { elements } => Type::Tuple {
            elements: elements
                .iter()
                .map(|e| expand_inline_aliases(e, inline, targets))
                .collect(),
        },
        other => other.clone(),
    }
}

fn normalize_value(
    typ: &Type,
    expr: &ValueExpr,
    enums: &HashMap<(String, String), (EnumIndexEntry, SourceLocation)>,
    current_namespace: &str,
    path: &std::path::Path,
    diags: &mut Diagnostics,
) -> Option<Value> {
    let span = expr.span;
    match typ {
        Type::I32 | Type::I64 | Type::U32 | Type::U64 => {
            // Pull out (value, suffix) from the literal, handling negation
            // for both suffix-bearing and plain forms.
            let (raw, suffix) = match &expr.kind {
                ValueExprKind::Int { value, suffix } => (*value, suffix.clone()),
                ValueExprKind::Neg(inner) => match &inner.kind {
                    ValueExprKind::Int { value, suffix } => (-(*value), suffix.clone()),
                    _ => {
                        diags.add(Diagnostic::error(
                            &source_loc(path, span),
                            "type-mismatch",
                            format!("expected integer, got {}", describe_value(&expr.kind)),
                        ));
                        return None;
                    }
                },
                _ => {
                    diags.add(Diagnostic::error(
                        &source_loc(path, span),
                        "type-mismatch",
                        format!("expected integer, got {}", describe_value(&expr.kind)),
                    ));
                    return None;
                }
            };

            // RFC 0004 §1: byte-size suffixes (B, KiB, MiB, …) are sugar
            // on integer literals. Other suffixes (s, ms, …) belong to
            // duration and are an error here.
            let n = match suffix {
                None => raw,
                Some(s) => match byte_suffix_multiplier(&s) {
                    Some(mul) => match raw.checked_mul(mul) {
                        Some(v) => v,
                        None => {
                            diags.add(Diagnostic::error(
                                &source_loc(path, span),
                                "out-of-range",
                                format!("`{}{}` overflows during multiplication", raw, s),
                            ));
                            return None;
                        }
                    },
                    None => {
                        diags.add(Diagnostic::error(
                            &source_loc(path, span),
                            "type-mismatch",
                            format!(
                                "suffix `{}` is not valid on integer literal (use a byte-size suffix: B, KB, MB, GB, TB, KiB, MiB, GiB, TiB)",
                                s
                            ),
                        ));
                        return None;
                    }
                },
            };

            // RFC 0004 §3: bounds-check the (possibly multiplied) value.
            let (min, max, name) = int_range_for_type(typ).unwrap();
            if n < min || n > max {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "out-of-range",
                    format!(
                        "value {} does not fit in {} (range: {}..={})",
                        n, name, min, max,
                    ),
                ));
                return None;
            }
            Some(Value::Integer(n))
        }
        Type::F32 | Type::F64 => match &expr.kind {
            ValueExprKind::Float { value, suffix: None } => Some(Value::Float(*value)),
            ValueExprKind::Int { value, suffix: None } => Some(Value::Float(*value as f64)),
            // Percentage sugar: `50%` → 0.5. Divides by 100, accepting both
            // integer (`50%`) and float (`12.5%`) literals.
            ValueExprKind::Float { value, suffix: Some(s) } if s == "%" => {
                Some(Value::Float(*value / 100.0))
            }
            ValueExprKind::Int { value, suffix: Some(s) } if s == "%" => {
                Some(Value::Float(*value as f64 / 100.0))
            }
            ValueExprKind::Float { suffix: Some(s), .. }
            | ValueExprKind::Int { suffix: Some(s), .. } => {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "type-mismatch",
                    format!(
                        "suffix `{}` is not valid on float type (only `%` is accepted)",
                        s
                    ),
                ));
                None
            }
            ValueExprKind::Neg(inner) => match &inner.kind {
                ValueExprKind::Float { value, suffix: None } => Some(Value::Float(-*value)),
                ValueExprKind::Int { value, suffix: None } => Some(Value::Float(-(*value as f64))),
                ValueExprKind::Float { value, suffix: Some(s) } if s == "%" => {
                    Some(Value::Float(-*value / 100.0))
                }
                ValueExprKind::Int { value, suffix: Some(s) } if s == "%" => {
                    Some(Value::Float(-(*value as f64) / 100.0))
                }
                _ => {
                    diags.add(Diagnostic::error(
                        &source_loc(path, span),
                        "type-mismatch",
                        "expected number".to_string(),
                    ));
                    None
                }
            },
            _ => {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "type-mismatch",
                    format!("expected number, got {}", describe_value(&expr.kind)),
                ));
                None
            }
        },
        Type::Bool => match &expr.kind {
            ValueExprKind::Bool(b) => Some(Value::Bool(*b)),
            _ => {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "type-mismatch",
                    format!("expected boolean, got {}", describe_value(&expr.kind)),
                ));
                None
            }
        },
        Type::String => match &expr.kind {
            ValueExprKind::Str(s) => Some(Value::String(s.clone())),
            _ => {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "type-mismatch",
                    format!("expected string, got {}", describe_value(&expr.kind)),
                ));
                None
            }
        },
        Type::Duration => duration_from_expr(expr, path, diags),
        Type::Regex => match &expr.kind {
            ValueExprKind::Str(s) => match validate_regex(s) {
                Ok(()) => Some(Value::String(s.clone())),
                Err(e) => {
                    diags.add(Diagnostic::error(
                        &source_loc(path, span),
                        "parse-failure",
                        e.to_string(),
                    ));
                    None
                }
            },
            _ => {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "type-mismatch",
                    "regex must be a string literal".to_string(),
                ));
                None
            }
        },
        Type::Url => match &expr.kind {
            ValueExprKind::Str(s) => match validate_url(s) {
                Ok(()) => Some(Value::String(s.clone())),
                Err(e) => {
                    diags.add(Diagnostic::error(
                        &source_loc(path, span),
                        "parse-failure",
                        e.to_string(),
                    ));
                    None
                }
            },
            _ => {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "type-mismatch",
                    "url must be a string literal".to_string(),
                ));
                None
            }
        },
        Type::Array { element } => match &expr.kind {
            ValueExprKind::Array { items, .. } => {
                let mut vals = Vec::new();
                for item in items {
                    vals.push(normalize_value(
                        element,
                        item,
                        enums,
                        current_namespace,
                        path,
                        diags,
                    )?);
                }
                Some(Value::Array(vals))
            }
            _ => {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "type-mismatch",
                    "expected array literal".to_string(),
                ));
                None
            }
        },
        Type::FixedArray { element, length } => match &expr.kind {
            ValueExprKind::Array { items, .. } => {
                let length_ok = items.len() as u32 == *length;
                if !length_ok {
                    diags.add(Diagnostic::error(
                        &source_loc(path, span),
                        "length-mismatch",
                        format!(
                            "expected {} elements for array<_, {}>, got {}",
                            length,
                            length,
                            items.len()
                        ),
                    ));
                }
                // Validate every element regardless of length so per-element
                // type errors aren't hidden by the length error.
                let mut vals = Vec::new();
                let mut element_ok = true;
                for item in items {
                    match normalize_value(element, item, enums, current_namespace, path, diags) {
                        Some(v) => vals.push(v),
                        None => element_ok = false,
                    }
                }
                if length_ok && element_ok {
                    Some(Value::Array(vals))
                } else {
                    None
                }
            }
            _ => {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "type-mismatch",
                    "expected array literal".to_string(),
                ));
                None
            }
        },
        Type::Map { value: vtyp, .. } => match &expr.kind {
            ValueExprKind::Map { entries, .. } => {
                let mut map = HashMap::new();
                for (key, val) in entries {
                    let k = match &key.kind {
                        MapKeyKind::Str(s) => s.clone(),
                        MapKeyKind::Ident(s) => s.clone(),
                        MapKeyKind::Int(n) => n.to_string(),
                    };
                    map.insert(
                        k,
                        normalize_value(vtyp, val, enums, current_namespace, path, diags)?,
                    );
                }
                Some(Value::Map(map))
            }
            _ => {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "type-mismatch",
                    "expected map literal".to_string(),
                ));
                None
            }
        },
        Type::Tuple { elements } => match &expr.kind {
            // RFC 0003 §1: tuple values are written `[...]`. The parser
            // produces `Array` for ordered literals; lowering disambiguates
            // against the declared type.
            ValueExprKind::Array { items, .. } | ValueExprKind::Tuple { items, .. } => {
                if items.len() != elements.len() {
                    diags.add(Diagnostic::error(
                        &source_loc(path, span),
                        "type-mismatch",
                        format!(
                            "tuple expects {} elements, got {}",
                            elements.len(),
                            items.len()
                        ),
                    ));
                    return None;
                }
                let mut vals = Vec::new();
                for (item_typ, item_expr) in elements.iter().zip(items.iter()) {
                    vals.push(normalize_value(
                        item_typ,
                        item_expr,
                        enums,
                        current_namespace,
                        path,
                        diags,
                    )?);
                }
                Some(Value::Tuple(vals))
            }
            _ => {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "type-mismatch",
                    "expected tuple literal `[...]`".to_string(),
                ));
                None
            }
        },
        Type::Optional { inner } => match &expr.kind {
            ValueExprKind::None_ => Some(Value::Optional(None)),
            _ => Some(Value::Optional(Some(Box::new(normalize_value(
                inner,
                expr,
                enums,
                current_namespace,
                path,
                diags,
            )?)))),
        },
        Type::Enum { name, .. } => match &expr.kind {
            ValueExprKind::Path { path: segs } => {
                // Allow either a bare variant name or `Enum::Variant` or `ns::Enum::Variant`.
                let variant = segs.last().unwrap().clone();
                // Find enum in any namespace where the name matches
                let lookup = enums
                    .iter()
                    .find(|((_, n), _)| n == name)
                    .map(|(_, (entry, _))| entry);
                match lookup {
                    Some(entry) => {
                        if entry.variants.iter().any(|v| v == &variant) {
                            let value = if entry.int_backed {
                                let _ = entry; // silence
                                // Find variant index for integer-backed
                                // (the index isn't our authority; use 0 and rely on generators using the name)
                                Value::Integer(0)
                            } else {
                                Value::String(variant.clone())
                            };
                            Some(Value::Enum {
                                variant,
                                value: Box::new(value),
                            })
                        } else {
                            diags.add(Diagnostic::error(
                                &source_loc(path, span),
                                "invalid-enum-variant",
                                format!("`{}` is not a variant of enum `{}`", variant, name),
                            ));
                            None
                        }
                    }
                    None => {
                        diags.add(Diagnostic::error(
                            &source_loc(path, span),
                            "unknown-type",
                            format!("enum `{}` is not declared", name),
                        ));
                        None
                    }
                }
            }
            _ => {
                diags.add(Diagnostic::error(
                    &source_loc(path, span),
                    "type-mismatch",
                    format!("expected an `{}` variant", name),
                ));
                None
            }
        },
        Type::Alias { .. } => {
            // Aliases at this point should only appear when the target was not an inlinable
            // primitive. Treat it as the underlying target via an upstream lookup. We don't
            // get here because callers expand the alias type to its target before normalizing.
            diags.add(Diagnostic::error(
                &source_loc(path, span),
                "internal-error",
                "alias target was not resolved before value normalization".to_string(),
            ));
            None
        }
        Type::Struct { .. } => {
            // Structs are not user-constructible in v1 of the DSL.
            diags.add(Diagnostic::error(
                &source_loc(path, span),
                "type-mismatch",
                "struct values are not supported in v1".to_string(),
            ));
            None
        }
    }
}

fn duration_from_expr(
    expr: &ValueExpr,
    path: &std::path::Path,
    diags: &mut Diagnostics,
) -> Option<Value> {
    match &expr.kind {
        ValueExprKind::Int {
            value,
            suffix: Some(s),
        } => duration_value(*value as f64, s, expr.span, path, diags),
        ValueExprKind::Float {
            value,
            suffix: Some(s),
        } => duration_value(*value, s, expr.span, path, diags),
        ValueExprKind::Str(s) => match parse_duration(s) {
            Ok(nanos) => Some(Value::Duration { nanoseconds: nanos }),
            Err(e) => {
                diags.add(Diagnostic::error(
                    &source_loc(path, expr.span),
                    "parse-failure",
                    e.to_string(),
                ));
                None
            }
        },
        _ => {
            diags.add(Diagnostic::error(
                &source_loc(path, expr.span),
                "type-mismatch",
                "duration requires a value with a unit suffix (e.g. 30s, 500ms)".to_string(),
            ));
            None
        }
    }
}

fn duration_value(
    value: f64,
    suffix: &str,
    span: Span,
    path: &std::path::Path,
    diags: &mut Diagnostics,
) -> Option<Value> {
    let nanos: f64 = match suffix {
        "ns" => value,
        "us" | "µs" => value * 1_000.0,
        "ms" => value * 1_000_000.0,
        "s" => value * 1_000_000_000.0,
        "min" | "m" => value * 60.0 * 1_000_000_000.0,
        "h" => value * 60.0 * 60.0 * 1_000_000_000.0,
        "d" => value * 24.0 * 60.0 * 60.0 * 1_000_000_000.0,
        "w" => value * 7.0 * 24.0 * 60.0 * 60.0 * 1_000_000_000.0,
        _ => {
            diags.add(Diagnostic::error(
                &source_loc(path, span),
                "parse-failure",
                format!(
                    "unknown duration unit `{}` (expected ns, us, ms, s, min, h, d, w)",
                    suffix
                ),
            ));
            return None;
        }
    };
    if nanos < 0.0 {
        diags.add(Diagnostic::error(
            &source_loc(path, span),
            "type-mismatch",
            "duration cannot be negative".to_string(),
        ));
        return None;
    }
    Some(Value::Duration {
        nanoseconds: nanos as u64,
    })
}

/// RFC 0004 §1: byte-size unit suffixes are now sugar on integer literals.
/// Returns the multiplier for a recognized suffix, or `None` for unknown.
fn byte_suffix_multiplier(suffix: &str) -> Option<i128> {
    Some(match suffix {
        "B" => 1,
        "KB" => 1_000,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        "TB" => 1_000_000_000_000,
        "KiB" => 1024,
        "MiB" => 1024 * 1024,
        "GiB" => 1024 * 1024 * 1024,
        "TiB" => 1024_i128 * 1024 * 1024 * 1024,
        _ => return None,
    })
}

fn describe_value(kind: &ValueExprKind) -> String {
    match kind {
        ValueExprKind::Int { .. } => "integer".into(),
        ValueExprKind::Float { .. } => "float".into(),
        ValueExprKind::Bool(_) => "bool".into(),
        ValueExprKind::Str(_) => "string".into(),
        ValueExprKind::None_ => "none".into(),
        ValueExprKind::Path { .. } => "identifier".into(),
        ValueExprKind::Array { .. } => "array".into(),
        ValueExprKind::Map { .. } => "map".into(),
        ValueExprKind::Tuple { .. } => "tuple".into(),
        ValueExprKind::Neg(_) => "negative number".into(),
    }
}
