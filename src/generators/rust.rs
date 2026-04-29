//! Rust code generator.
//!
//! Emits a single `.rs` file with one `pub mod <namespace> { ... }` block
//! per source-file namespace. Cross-namespace references (e.g. a constant
//! in `limits` typed as `network::Port`) are emitted as `super::network::Port`.

use crate::ir::{CodeGenRequest, CodeGenResponse, EnumDef, GeneratedFile, Module, SymbolMapping, TypeAliasDef};
use crate::types::{
    escape_keyword, resolve_alias, to_pascal_case, to_screaming_snake_case, Type, Value,
    RUST_KEYWORDS,
};
use super::Generator;
use std::collections::HashMap;

const INDENT: &str = "    ";

struct LineTracker {
    content: String,
    current_line: u32,
    current_column: u32,
    mappings: Vec<SymbolMapping>,
}

impl LineTracker {
    fn new() -> Self {
        Self {
            content: String::new(),
            current_line: 1,
            current_column: 1,
            mappings: Vec::new(),
        }
    }

    fn push_str(&mut self, s: &str) {
        self.content.push_str(s);
        for c in s.chars() {
            if c == '\n' {
                self.current_line += 1;
                self.current_column = 1;
            } else {
                self.current_column += 1;
            }
        }
    }

    fn add_mapping(&mut self, symbol: String, column: u32) {
        self.mappings.push(SymbolMapping {
            symbol,
            line: self.current_line,
            column,
        });
    }

    fn into_parts(self) -> (String, Vec<SymbolMapping>) {
        (self.content, self.mappings)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub enum Visibility {
    #[default]
    Pub,
    PubCrate,
    PubSuper,
    Private,
}

impl Visibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Pub => "pub ",
            Visibility::PubCrate => "pub(crate) ",
            Visibility::PubSuper => "pub(super) ",
            Visibility::Private => "",
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RustGenerator {
    pub visibility: Visibility,
}

impl RustGenerator {
    pub fn from_options(options: &HashMap<String, serde_json::Value>) -> Self {
        let mut generator = Self::default();

        if let Some(serde_json::Value::String(s)) = options.get("visibility") {
            generator.visibility = match s.as_str() {
                "pub(crate)" => Visibility::PubCrate,
                "pub(super)" => Visibility::PubSuper,
                "" => Visibility::Private,
                _ => Visibility::Pub,
            };
        }

        generator
    }

    fn convert_name(&self, name: &str) -> String {
        let converted = to_screaming_snake_case(name);
        escape_keyword(&converted, RUST_KEYWORDS)
    }

    /// Build the cross-namespace path for an enum/alias reference. When the
    /// type is in the same namespace as the surrounding module body, returns
    /// just the name. Otherwise, prepends `super::<ns>::`.
    fn qualify(&self, name: &str, ns: &str, current_ns: &str) -> String {
        let pascal = to_pascal_case(name);
        if ns.is_empty() || ns == current_ns {
            pascal
        } else {
            format!("super::{}::{}", ns, pascal)
        }
    }

    fn generate_type(&self, typ: &Type, current_ns: &str) -> String {
        match typ {
            Type::I32 => "i32".to_string(),
            Type::I64 => "i64".to_string(),
            Type::U32 => "u32".to_string(),
            Type::U64 => "u64".to_string(),
            Type::F32 => "f32".to_string(),
            Type::F64 => "f64".to_string(),
            Type::Bool => "bool".to_string(),
            Type::String | Type::Regex | Type::Url => "&'static str".to_string(),
            Type::Duration => "std::time::Duration".to_string(),
            Type::Array { element } => format!("&'static [{}]", self.generate_type(element, current_ns)),
            Type::FixedArray { element, length } => {
                format!("[{}; {}]", self.generate_type(element, current_ns), length)
            }
            Type::Map { .. } => {
                "&'static [(&'static str, &'static str)]".to_string()
            }
            Type::Tuple { elements } => {
                let types: Vec<_> = elements.iter().map(|e| self.generate_type(e, current_ns)).collect();
                format!("({})", types.join(", "))
            }
            Type::Optional { inner } => format!("Option<{}>", self.generate_type(inner, current_ns)),
            Type::Enum { name, namespace } => self.qualify(name, namespace, current_ns),
            Type::Alias { name, namespace } => self.qualify(name, namespace, current_ns),
            Type::Struct { fields } => {
                let mut field_types: Vec<_> = fields.iter().collect();
                field_types.sort_by_key(|(k, _)| k.as_str());
                let types: Vec<_> = field_types
                    .iter()
                    .map(|(_, v)| self.generate_type(v, current_ns))
                    .collect();
                format!("({})", types.join(", "))
            }
        }
    }

    fn generate_value(&self, value: &Value, typ: &Type, current_ns: &str) -> String {
        match value {
            Value::Integer(i) => i.to_string(),
            Value::Float(f) => {
                let s = f.to_string();
                if s.contains('.') { s } else { format!("{}.0", s) }
            }
            Value::Bool(b) => b.to_string(),
            Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
            Value::Duration { nanoseconds } => {
                format!("std::time::Duration::from_nanos({})", nanoseconds)
            }
            Value::Array(arr) => {
                let inner_type = match typ {
                    Type::Array { element } => element.as_ref(),
                    Type::FixedArray { element, .. } => element.as_ref(),
                    _ => &Type::String,
                };
                let items: Vec<_> = arr.iter().map(|v| self.generate_value(v, inner_type, current_ns)).collect();
                let prefix = if matches!(typ, Type::FixedArray { .. }) { "" } else { "&" };
                format!("{}[{}]", prefix, items.join(", "))
            }
            Value::Map(_) => "&[]".to_string(),
            Value::Tuple(vals) => {
                let elem_types = match typ {
                    Type::Tuple { elements } => elements.clone(),
                    _ => vec![Type::String; vals.len()],
                };
                let items: Vec<_> = vals
                    .iter()
                    .zip(elem_types.iter())
                    .map(|(v, t)| self.generate_value(v, t, current_ns))
                    .collect();
                format!("({})", items.join(", "))
            }
            Value::Optional(opt) => match opt {
                Some(v) => {
                    let inner = match typ {
                        Type::Optional { inner } => inner.as_ref(),
                        _ => &Type::String,
                    };
                    format!("Some({})", self.generate_value(v, inner, current_ns))
                }
                None => "None".to_string(),
            },
            Value::Enum { variant, value: _ } => {
                let qualified = match typ {
                    Type::Enum { name, namespace } => self.qualify(name, namespace, current_ns),
                    _ => "Enum".to_string(),
                };
                format!("{}::{}", qualified, to_pascal_case(variant))
            }
            Value::Struct(fields) => {
                let field_types = match typ {
                    Type::Struct { fields: f } => f.clone(),
                    _ => HashMap::new(),
                };
                let mut field_list: Vec<_> = fields.iter().collect();
                field_list.sort_by_key(|(k, _)| k.as_str());
                let items: Vec<_> = field_list
                    .iter()
                    .map(|(k, v)| {
                        let ft = field_types.get(*k).unwrap_or(&Type::String);
                        self.generate_value(v, ft, current_ns)
                    })
                    .collect();
                format!("({})", items.join(", "))
            }
        }
    }

    fn emit_alias(&self, tracker: &mut LineTracker, alias: &TypeAliasDef, indent: &str, current_ns: &str) {
        if let Some(ref doc) = alias.doc {
            for line in doc.lines() {
                tracker.push_str(&format!("{}/// {}\n", indent, line));
            }
        }
        let name = to_pascal_case(&alias.name);
        let target = self.generate_type(&alias.target, current_ns);
        let column = (indent.len() as u32) + (self.visibility.as_str().len() as u32) + 6;
        tracker.add_mapping(format!("{}.{}", alias.namespace, alias.name), column);
        tracker.push_str(&format!("{}{}type {} = {};\n\n", indent, self.visibility.as_str(), name, target));
    }

    fn emit_enum(&self, tracker: &mut LineTracker, enum_def: &EnumDef, indent: &str, _current_ns: &str) {
        if let Some(ref doc) = enum_def.doc {
            for line in doc.lines() {
                tracker.push_str(&format!("{}/// {}\n", indent, line));
            }
        }
        tracker.push_str(&format!("{}#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n", indent));
        if enum_def.backing_type == "integer" {
            tracker.push_str(&format!("{}#[repr(i32)]\n", indent));
        }

        let name = to_pascal_case(&enum_def.name);
        let column = (indent.len() as u32) + (self.visibility.as_str().len() as u32) + 6;
        tracker.add_mapping(format!("{}.{}", enum_def.namespace, enum_def.name), column);
        tracker.push_str(&format!("{}{}enum {} {{\n", indent, self.visibility.as_str(), name));

        for variant in &enum_def.variants {
            let variant_name = to_pascal_case(&variant.name);
            match &variant.value {
                Value::Integer(i) => {
                    tracker.push_str(&format!("{}{}{} = {},\n", indent, INDENT, variant_name, i));
                }
                _ => {
                    tracker.push_str(&format!("{}{}{},\n", indent, INDENT, variant_name));
                }
            }
        }
        tracker.push_str(&format!("{}}}\n\n", indent));

        if enum_def.backing_type == "string" {
            tracker.push_str(&format!("{}impl {} {{\n", indent, name));
            tracker.push_str(&format!("{}{}{}fn as_str(&self) -> &'static str {{\n", indent, INDENT, self.visibility.as_str()));
            tracker.push_str(&format!("{}{}{}match self {{\n", indent, INDENT, INDENT));
            for variant in &enum_def.variants {
                let variant_name = to_pascal_case(&variant.name);
                if let Value::String(s) = &variant.value {
                    tracker.push_str(&format!(
                        "{}{}{}{}{}::{} => \"{}\",\n",
                        indent, INDENT, INDENT, INDENT,
                        name, variant_name, s,
                    ));
                }
            }
            tracker.push_str(&format!("{}{}{}}}\n", indent, INDENT, INDENT));
            tracker.push_str(&format!("{}{}}}\n", indent, INDENT));
            tracker.push_str(&format!("{}}}\n\n", indent));
        }
    }

    fn emit_constant(&self, tracker: &mut LineTracker, module: &Module, constant: &crate::ir::Constant, alias_map: &HashMap<String, Type>, indent: &str) {
        let current_ns = module.namespace.as_str();
        if let Some(ref doc) = constant.doc {
            for line in doc.lines() {
                tracker.push_str(&format!("{}/// {}\n", indent, line));
            }
        }

        let name = self.convert_name(&constant.name);

        // For struct types, generate a struct definition first, then init it.
        let (typ, value) = if let Type::Struct { fields } = &constant.typ {
            let struct_name = to_pascal_case(&constant.name);

            tracker.push_str(&format!("{}#[derive(Debug, Clone, Copy, PartialEq)]\n", indent));
            tracker.push_str(&format!("{}{}struct {} {{\n", indent, self.visibility.as_str(), struct_name));

            let mut field_list: Vec<_> = fields.iter().collect();
            field_list.sort_by_key(|(k, _)| k.as_str());

            for (field_name, field_type) in &field_list {
                tracker.push_str(&format!(
                    "{}{}{}{}: {},\n",
                    indent, INDENT, self.visibility.as_str(), field_name,
                    self.generate_type(field_type, current_ns)
                ));
            }
            tracker.push_str(&format!("{}}}\n\n", indent));

            let struct_value = if let Value::Struct(value_fields) = &constant.value {
                let field_inits: Vec<_> = field_list
                    .iter()
                    .map(|(k, t)| {
                        let v = value_fields.get(*k).unwrap();
                        format!("{}: {}", k, self.generate_value(v, t, current_ns))
                    })
                    .collect();
                format!("{} {{ {} }}", struct_name, field_inits.join(", "))
            } else {
                format!("{} {{ }}", struct_name)
            };

            (struct_name, struct_value)
        } else {
            let resolved = resolve_alias(&constant.typ, alias_map);
            (
                self.generate_type(&constant.typ, current_ns),
                self.generate_value(&constant.value, &resolved, current_ns),
            )
        };

        let column = (indent.len() as u32) + (self.visibility.as_str().len() as u32) + 7;
        tracker.add_mapping(format!("{}.{}", module.namespace, constant.name), column);
        tracker.push_str(&format!(
            "{}{}const {}: {} = {};\n\n",
            indent, self.visibility.as_str(), name, typ, value,
        ));
    }
}

impl Generator for RustGenerator {
    fn generate(&self, request: &CodeGenRequest) -> CodeGenResponse {
        let mut tracker = LineTracker::new();

        // Prelude (user-defined, e.g. lint exclusions)
        if let Some(prelude) = request.options.get("prelude").and_then(|v| v.as_str()) {
            tracker.push_str(prelude);
            if !prelude.ends_with('\n') {
                tracker.push_str("\n");
            }
        }

        tracker.push_str("// Generated by primate\n// Do not edit manually.\n\n");

        let alias_map: HashMap<String, Type> = request
            .aliases
            .iter()
            .map(|a| (a.name.clone(), a.target.clone()))
            .collect();

        // Collect every namespace that appears anywhere — modules, enums,
        // aliases. A namespace can have aliases or enums but no constants
        // (and vice versa).
        let mut all_namespaces: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for module in &request.modules {
            if seen.insert(module.namespace.clone()) {
                all_namespaces.push(module.namespace.clone());
            }
        }
        for e in &request.enums {
            if seen.insert(e.namespace.clone()) {
                all_namespaces.push(e.namespace.clone());
            }
        }
        for a in &request.aliases {
            if seen.insert(a.namespace.clone()) {
                all_namespaces.push(a.namespace.clone());
            }
        }
        all_namespaces.sort();

        for ns in &all_namespaces {
            let module = request.modules.iter().find(|m| &m.namespace == ns);
            let enums: Vec<&EnumDef> = request.enums.iter().filter(|e| &e.namespace == ns).collect();
            let aliases: Vec<&TypeAliasDef> = request.aliases.iter().filter(|a| &a.namespace == ns).collect();

            if let Some(module) = module {
                tracker.push_str(&format!("// source: {}\n", module.source_file));
                if let Some(ref doc) = module.doc {
                    for line in doc.lines() {
                        tracker.push_str(&format!("//! {}\n", line));
                    }
                }
            }
            tracker.push_str(&format!("{}mod {} {{\n", self.visibility.as_str(), ns));

            for alias in &aliases {
                self.emit_alias(&mut tracker, alias, INDENT, ns);
            }

            for enum_def in &enums {
                self.emit_enum(&mut tracker, enum_def, INDENT, ns);
            }

            if let Some(module) = module {
                for constant in &module.constants {
                    self.emit_constant(&mut tracker, module, constant, &alias_map, INDENT);
                }
            }

            tracker.push_str("}\n\n");
        }

        let (content, mappings) = tracker.into_parts();

        CodeGenResponse {
            files: vec![GeneratedFile {
                path: request.output_path.clone(),
                content,
                mappings,
            }],
            errors: vec![],
        }
    }

    fn name(&self) -> &'static str {
        "rust"
    }
}
