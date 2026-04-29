//! Python code generator.
//!
//! Always emits a directory: one `.py` file per source-file namespace, plus
//! an `__init__.py` that re-exports each namespace as a submodule. Cross-
//! namespace type references become relative imports (`from .other import X`).

use super::Generator;
use crate::ir::{
    CodeGenRequest, CodeGenResponse, EnumDef, GeneratedFile, Module, SymbolMapping, TypeAliasDef,
};
use crate::types::{Type, Value, resolve_alias, to_pascal_case, to_screaming_snake_case};
use std::collections::{BTreeMap, BTreeSet, HashMap};

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

#[derive(Debug, Default)]
pub struct PythonGenerator {
    // r[impl gen.python.typing-option]
    pub typing: TypingStyle,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum TypingStyle {
    #[default]
    Runtime,
    Stub,
}

impl PythonGenerator {
    pub fn from_options(options: &HashMap<String, serde_json::Value>) -> Self {
        let mut generator = Self::default();

        if let Some(serde_json::Value::String(s)) = options.get("typing") {
            generator.typing = match s.as_str() {
                "stub" => TypingStyle::Stub,
                _ => TypingStyle::Runtime,
            };
        }

        generator
    }

    /// Walk a type and record cross-namespace enum/alias references.
    fn collect_imports(
        &self,
        typ: &Type,
        current_ns: &str,
        imports: &mut BTreeMap<String, BTreeSet<String>>,
    ) {
        match typ {
            Type::Enum { name, namespace } | Type::Alias { name, namespace } => {
                if !namespace.is_empty() && namespace != current_ns {
                    imports
                        .entry(namespace.clone())
                        .or_default()
                        .insert(to_pascal_case(name));
                }
            }
            Type::Array { element } => self.collect_imports(element, current_ns, imports),
            Type::FixedArray { element, .. } => self.collect_imports(element, current_ns, imports),
            Type::Optional { inner } => self.collect_imports(inner, current_ns, imports),
            Type::Map { key, value } => {
                self.collect_imports(key, current_ns, imports);
                self.collect_imports(value, current_ns, imports);
            }
            Type::Tuple { elements } => {
                for e in elements {
                    self.collect_imports(e, current_ns, imports);
                }
            }
            Type::Struct { fields } => {
                for v in fields.values() {
                    self.collect_imports(v, current_ns, imports);
                }
            }
            _ => {}
        }
    }

    fn generate_type(&self, typ: &Type) -> String {
        match typ {
            Type::I32 | Type::I64 | Type::U32 | Type::U64 => "int".to_string(),
            Type::F32 | Type::F64 => "float".to_string(),
            Type::Bool => "bool".to_string(),
            Type::String | Type::Regex | Type::Url => "str".to_string(),
            Type::Duration => "timedelta".to_string(),
            Type::Array { element } => format!("List[{}]", self.generate_type(element)),
            Type::FixedArray { element, length } => {
                let parts = vec![self.generate_type(element); *length as usize];
                format!("Tuple[{}]", parts.join(", "))
            }
            Type::Map { key, value } => {
                format!(
                    "Dict[{}, {}]",
                    self.generate_type(key),
                    self.generate_type(value)
                )
            }
            Type::Tuple { elements } => {
                let types: Vec<_> = elements.iter().map(|e| self.generate_type(e)).collect();
                format!("Tuple[{}]", types.join(", "))
            }
            Type::Optional { inner } => format!("Optional[{}]", self.generate_type(inner)),
            Type::Enum { name, .. } => to_pascal_case(name),
            Type::Alias { name, .. } => to_pascal_case(name),
            Type::Struct { fields } => {
                let field_strs: Vec<_> = fields
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, self.generate_type(v)))
                    .collect();
                format!("Dict[str, Any]  # {{ {} }}", field_strs.join(", "))
            }
        }
    }

    fn generate_value(&self, value: &Value, typ: &Type) -> String {
        match value {
            Value::Integer(i) => i.to_string(),
            Value::Float(f) => {
                if f.is_nan() {
                    "float('nan')".to_string()
                } else if f.is_infinite() {
                    if *f > 0.0 {
                        "float('inf')"
                    } else {
                        "float('-inf')"
                    }
                    .to_string()
                } else {
                    f.to_string()
                }
            }
            Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
            Value::Duration { nanoseconds } => {
                let seconds = *nanoseconds as f64 / 1_000_000_000.0;
                format!("timedelta(seconds={})", seconds)
            }
            Value::Array(arr) => {
                let inner_type = match typ {
                    Type::Array { element } => element.as_ref(),
                    Type::FixedArray { element, .. } => element.as_ref(),
                    _ => &Type::String,
                };
                let items: Vec<_> = arr
                    .iter()
                    .map(|v| self.generate_value(v, inner_type))
                    .collect();
                format!("[{}]", items.join(", "))
            }
            Value::Map(map) => {
                let val_type = match typ {
                    Type::Map { value, .. } => value.as_ref(),
                    _ => &Type::String,
                };
                let entries: Vec<_> = map
                    .iter()
                    .map(|(k, v)| format!("\"{}\": {}", k, self.generate_value(v, val_type)))
                    .collect();
                format!("{{ {} }}", entries.join(", "))
            }
            Value::Tuple(vals) => {
                let elem_types = match typ {
                    Type::Tuple { elements } => elements.clone(),
                    _ => vec![Type::String; vals.len()],
                };
                let items: Vec<_> = vals
                    .iter()
                    .zip(elem_types.iter())
                    .map(|(v, t)| self.generate_value(v, t))
                    .collect();
                format!("({})", items.join(", "))
            }
            Value::Optional(opt) => match opt {
                Some(v) => {
                    let inner = match typ {
                        Type::Optional { inner } => inner.as_ref(),
                        _ => &Type::String,
                    };
                    self.generate_value(v, inner)
                }
                None => "None".to_string(),
            },
            Value::Enum { variant, .. } => {
                let enum_name = match typ {
                    Type::Enum { name, .. } => to_pascal_case(name),
                    _ => "Enum".to_string(),
                };
                // r[impl type.enum.variant-naming.python]
                format!("{}.{}", enum_name, to_screaming_snake_case(variant))
            }
            Value::Struct(fields) => {
                let field_types = match typ {
                    Type::Struct { fields } => fields.clone(),
                    _ => HashMap::new(),
                };
                let entries: Vec<_> = fields
                    .iter()
                    .map(|(k, v)| {
                        let ft = field_types.get(k).unwrap_or(&Type::String);
                        format!("\"{}\": {}", k, self.generate_value(v, ft))
                    })
                    .collect();
                format!("{{ {} }}", entries.join(", "))
            }
        }
    }

    fn generate_enum(&self, enum_def: &EnumDef) -> String {
        let mut output = String::new();
        let name = to_pascal_case(&enum_def.name);

        if let Some(ref doc) = enum_def.doc {
            output.push_str(&format!("\"\"\"\n{}\n\"\"\"\n", doc));
        }

        let base_class = if enum_def.backing_type == "integer" {
            "IntEnum"
        } else {
            "str, Enum"
        };

        output.push_str(&format!("class {}({}):\n", name, base_class));

        for variant in &enum_def.variants {
            let variant_name = to_screaming_snake_case(&variant.name);
            let val_type = if enum_def.backing_type == "integer" {
                Type::I32
            } else {
                Type::String
            };
            output.push_str(&format!(
                "    {} = {}\n",
                variant_name,
                self.generate_value(&variant.value, &val_type)
            ));
        }
        output.push_str("\n\n");

        output
    }

    fn emit_alias(&self, tracker: &mut LineTracker, alias: &TypeAliasDef) {
        if let Some(ref doc) = alias.doc {
            tracker.push_str(&format!("\"\"\"\n{}\n\"\"\"\n", doc));
        }
        let name = to_pascal_case(&alias.name);
        let target = self.generate_type(&alias.target);
        tracker.add_mapping(format!("{}.{}", alias.namespace, alias.name), 1);
        tracker.push_str(&format!("{}: TypeAlias = {}\n\n", name, target));
    }

    fn generate_module_file(
        &self,
        module: Option<&Module>,
        namespace: &str,
        source_file: Option<&str>,
        enums: &[&EnumDef],
        aliases: &[&TypeAliasDef],
        alias_map: &HashMap<String, Type>,
        prelude: Option<&str>,
    ) -> (String, Vec<SymbolMapping>) {
        // Collect cross-namespace imports.
        let mut imports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for alias in aliases {
            self.collect_imports(&alias.target, namespace, &mut imports);
        }
        if let Some(module) = module {
            for c in &module.constants {
                self.collect_imports(&c.typ, namespace, &mut imports);
            }
        }

        let mut tracker = LineTracker::new();

        if let Some(prelude) = prelude {
            tracker.push_str(prelude);
            if !prelude.ends_with('\n') {
                tracker.push_str("\n");
            }
        }

        if let Some(source) = source_file {
            tracker.push_str(&format!("# Generated by primate from {}\n", source));
        } else {
            tracker.push_str("# Generated by primate\n");
        }
        tracker.push_str("# Do not edit manually.\n\n");

        // Standard imports.
        tracker.push_str("from enum import Enum, IntEnum\n");
        tracker.push_str("from datetime import timedelta\n");
        tracker.push_str(
            "from typing import List, Dict, Tuple, Optional, Any, NamedTuple, TypeAlias\n",
        );

        // Cross-namespace imports.
        if !imports.is_empty() {
            tracker.push_str("\n");
            for (other_ns, names) in &imports {
                let names: Vec<_> = names.iter().cloned().collect();
                tracker.push_str(&format!("from .{} import {}\n", other_ns, names.join(", "),));
            }
        }
        tracker.push_str("\n");

        for alias in aliases {
            self.emit_alias(&mut tracker, alias);
        }

        for enum_def in enums {
            let column = 7; // "class " + 1
            tracker.add_mapping(format!("{}.{}", enum_def.namespace, enum_def.name), column);
            tracker.push_str(&self.generate_enum(enum_def));
        }

        if let Some(module) = module {
            for constant in &module.constants {
                if let Some(ref doc) = constant.doc {
                    tracker.push_str(&format!("\"\"\"\n{}\n\"\"\"\n", doc));
                }

                // For struct types, generate a NamedTuple class first.
                let (type_ann, value) = if let Type::Struct { fields } = &constant.typ {
                    let type_name = format!("{}Type", to_pascal_case(&constant.name));
                    tracker.push_str(&format!("class {}(NamedTuple):\n", type_name));
                    let mut field_list: Vec<_> = fields.iter().collect();
                    field_list.sort_by_key(|(k, _)| k.as_str());
                    for (field_name, field_type) in &field_list {
                        tracker.push_str(&format!(
                            "    {}: {}\n",
                            field_name,
                            self.generate_type(field_type)
                        ));
                    }
                    tracker.push_str("\n");

                    let struct_value = if let Value::Struct(value_fields) = &constant.value {
                        let field_inits: Vec<_> = field_list
                            .iter()
                            .map(|(k, t)| {
                                let v = value_fields.get(*k).unwrap();
                                format!("{}={}", k, self.generate_value(v, t))
                            })
                            .collect();
                        format!("{}({})", type_name, field_inits.join(", "))
                    } else {
                        format!("{}()", type_name)
                    };

                    (type_name, struct_value)
                } else {
                    let resolved = resolve_alias(&constant.typ, alias_map);
                    (
                        self.generate_type(&constant.typ),
                        self.generate_value(&constant.value, &resolved),
                    )
                };

                let column = 1;
                tracker.add_mapping(format!("{}.{}", module.namespace, constant.name), column);
                tracker.push_str(&format!("{}: {} = {}\n\n", constant.name, type_ann, value));
            }
        }

        tracker.into_parts()
    }

    fn generate_init_file(&self, namespaces: &[String], prelude: Option<&str>) -> String {
        let mut output = String::new();

        if let Some(prelude) = prelude {
            output.push_str(prelude);
            if !prelude.ends_with('\n') {
                output.push('\n');
            }
        }

        output.push_str("# Generated by primate\n");
        output.push_str("# Do not edit manually.\n\n");

        for ns in namespaces {
            output.push_str(&format!("from . import {}\n", ns));
        }
        output.push('\n');
        output.push_str("__all__ = [\n");
        for ns in namespaces {
            output.push_str(&format!("    \"{}\",\n", ns));
        }
        output.push_str("]\n");

        output
    }
}

impl Generator for PythonGenerator {
    fn generate(&self, request: &CodeGenRequest) -> CodeGenResponse {
        let mut files = Vec::new();
        let prelude = request.options.get("prelude").and_then(|v| v.as_str());

        let dir = request.output_path.trim_end_matches('/');

        let alias_map: HashMap<String, Type> = request
            .aliases
            .iter()
            .map(|a| (a.name.clone(), a.target.clone()))
            .collect();

        let mut all_namespaces: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for m in &request.modules {
            if seen.insert(m.namespace.clone()) {
                all_namespaces.push(m.namespace.clone());
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
            let enums: Vec<&EnumDef> = request
                .enums
                .iter()
                .filter(|e| &e.namespace == ns)
                .collect();
            let aliases: Vec<&TypeAliasDef> = request
                .aliases
                .iter()
                .filter(|a| &a.namespace == ns)
                .collect();
            let source_file = module.map(|m| m.source_file.as_str());

            let (content, mappings) = self.generate_module_file(
                module,
                ns,
                source_file,
                &enums,
                &aliases,
                &alias_map,
                prelude,
            );

            files.push(GeneratedFile {
                path: format!("{}/{}.py", dir, ns),
                content,
                mappings,
            });
        }

        let init_content = self.generate_init_file(&all_namespaces, prelude);
        files.push(GeneratedFile {
            path: format!("{}/__init__.py", dir),
            content: init_content,
            mappings: vec![],
        });

        CodeGenResponse {
            files,
            errors: vec![],
        }
    }

    fn name(&self) -> &'static str {
        "python"
    }
}
