//! TypeScript code generator.
//!
//! Always emits a directory: one `.ts` file per source-file namespace, plus
//! an `index.ts` that re-exports each namespace. Cross-namespace type
//! references become real ES `import` statements at the top of each file.

use super::Generator;
use crate::ir::{
    CodeGenRequest, CodeGenResponse, EnumDef, GeneratedFile, Module, SymbolMapping, TypeAliasDef,
};
use crate::types::{
    TS_KEYWORDS, Type, Value, escape_keyword, resolve_alias, to_camel_case, to_pascal_case,
};
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

#[derive(Debug, Default, Clone, Copy)]
pub struct TypeScriptGenerator {
    // r[impl gen.ts.naming-option]
    pub naming: NamingStyle,
    // r[impl gen.ts.duration-option]
    pub duration: DurationStyle,
    pub u64_style: U64Style,
    // r[impl gen.ts.enum-style-option]
    pub enum_style: EnumStyle,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum NamingStyle {
    #[default]
    CamelCase,
    ScreamingSnakeCase,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum DurationStyle {
    #[default]
    Number,
    Temporal,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum U64Style {
    #[default]
    Number,
    BigInt,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum EnumStyle {
    #[default]
    Literal,
    Const,
    Enum,
}

impl TypeScriptGenerator {
    pub fn from_options(options: &HashMap<String, serde_json::Value>) -> Self {
        let mut generator = Self::default();

        if let Some(serde_json::Value::String(s)) = options.get("naming") {
            generator.naming = match s.as_str() {
                "SCREAMING_SNAKE_CASE" => NamingStyle::ScreamingSnakeCase,
                _ => NamingStyle::CamelCase,
            };
        }

        if let Some(serde_json::Value::String(s)) = options.get("duration") {
            generator.duration = match s.as_str() {
                "temporal" => DurationStyle::Temporal,
                _ => DurationStyle::Number,
            };
        }

        if let Some(serde_json::Value::String(s)) = options.get("u64") {
            generator.u64_style = match s.as_str() {
                "bigint" => U64Style::BigInt,
                _ => U64Style::Number,
            };
        }

        if let Some(serde_json::Value::String(s)) = options.get("enumStyle") {
            generator.enum_style = match s.as_str() {
                "const" => EnumStyle::Const,
                "enum" => EnumStyle::Enum,
                _ => EnumStyle::Literal,
            };
        }

        generator
    }

    fn convert_name(&self, name: &str) -> String {
        let converted = match self.naming {
            NamingStyle::CamelCase => to_camel_case(name),
            NamingStyle::ScreamingSnakeCase => name.to_string(),
        };
        escape_keyword(&converted, TS_KEYWORDS)
    }

    /// Walk a type and record any cross-namespace enum/alias references for
    /// the importer in `imports` (keyed by source namespace → set of bare
    /// names to import).
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
            Type::I32 | Type::I64 | Type::U32 | Type::F32 | Type::F64 => "number".to_string(),
            Type::U64 => match self.u64_style {
                U64Style::BigInt => "bigint".to_string(),
                U64Style::Number => "number".to_string(),
            },
            Type::Bool => "boolean".to_string(),
            Type::String | Type::Regex | Type::Url => "string".to_string(),
            // r[impl type.duration.ts]
            Type::Duration => match self.duration {
                DurationStyle::Number => "number".to_string(),
                DurationStyle::Temporal => "Temporal.Duration".to_string(),
            },
            Type::Array { element } => format!("{}[]", self.generate_type(element)),
            Type::FixedArray { element, length } => {
                let parts = vec![self.generate_type(element); *length as usize];
                format!("[{}]", parts.join(", "))
            }
            Type::Map { key, value } => {
                format!(
                    "Record<{}, {}>",
                    self.generate_type(key),
                    self.generate_type(value)
                )
            }
            Type::Tuple { elements } => {
                let types: Vec<_> = elements.iter().map(|e| self.generate_type(e)).collect();
                format!("[{}]", types.join(", "))
            }
            Type::Optional { inner } => format!("{} | null", self.generate_type(inner)),
            Type::Enum { name, .. } => to_pascal_case(name),
            Type::Alias { name, .. } => to_pascal_case(name),
            Type::Struct { fields } => {
                let field_strs: Vec<_> = fields
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, self.generate_type(v)))
                    .collect();
                format!("{{ {} }}", field_strs.join("; "))
            }
        }
    }

    fn generate_value(&self, value: &Value, typ: &Type) -> String {
        match value {
            Value::Integer(i) => i.to_string(),
            Value::Float(f) => {
                if f.is_nan() {
                    "NaN".to_string()
                } else if f.is_infinite() {
                    if *f > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
                } else {
                    f.to_string()
                }
            }
            Value::Bool(b) => b.to_string(),
            Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
            // r[impl type.duration.ts]
            Value::Duration { nanoseconds } => {
                let millis = nanoseconds / 1_000_000;
                match self.duration {
                    DurationStyle::Number => millis.to_string(),
                    DurationStyle::Temporal => {
                        format!("Temporal.Duration.from({{ milliseconds: {} }})", millis)
                    }
                }
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
                format!("[{}]", items.join(", "))
            }
            Value::Optional(opt) => match opt {
                Some(v) => {
                    let inner = match typ {
                        Type::Optional { inner } => inner.as_ref(),
                        _ => &Type::String,
                    };
                    self.generate_value(v, inner)
                }
                None => "null".to_string(),
            },
            Value::Enum { variant, value } => {
                // Reference the enum object by name (`LogLevel.Warn`) so the
                // generated code carries the type at the use-site rather than
                // a bare literal. Cross-namespace enums get the import added
                // separately by `collect_imports`.
                if let Type::Enum { name, .. } = typ {
                    format!("{}.{}", to_pascal_case(name), variant)
                } else {
                    self.generate_value(value, &Type::String)
                }
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
                        format!("{}: {}", k, self.generate_value(v, ft))
                    })
                    .collect();
                format!("{{ {} }}", entries.join(", "))
            }
        }
    }

    /// Emit one `.ts` file for a single namespace. Imports for cross-namespace
    /// references are collected up-front and emitted at the top.
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
        // Walk all types we'll emit and collect cross-namespace imports.
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
            tracker.push_str(&format!("// Generated by primate from {}\n", source));
        } else {
            tracker.push_str("// Generated by primate\n");
        }
        tracker.push_str("// Do not edit manually.\n\n");

        // Imports.
        if !imports.is_empty() {
            for (other_ns, names) in &imports {
                let names: Vec<_> = names.iter().cloned().collect();
                tracker.push_str(&format!(
                    "import {{ {} }} from './{}';\n",
                    names.join(", "),
                    other_ns,
                ));
            }
            tracker.push_str("\n");
        }

        for alias in aliases {
            self.emit_alias(&mut tracker, alias);
        }

        for enum_def in enums {
            let column = tracker.current_column;
            tracker.add_mapping(format!("{}.{}", enum_def.namespace, enum_def.name), column);
            tracker.push_str(&self.generate_enum(enum_def));
        }

        if let Some(module) = module {
            for constant in &module.constants {
                if let Some(ref doc) = constant.doc {
                    tracker.push_str("/**\n");
                    for line in doc.lines() {
                        tracker.push_str(&format!(" * {}\n", line));
                    }
                    tracker.push_str(" */\n");
                }

                let name = self.convert_name(&constant.name);
                let resolved = resolve_alias(&constant.typ, alias_map);
                let value = self.generate_value(&constant.value, &resolved);

                let column = 14; // "export const ".len() + 1
                tracker.add_mapping(format!("{}.{}", module.namespace, constant.name), column);
                tracker.push_str(&format!("export const {} = {} as const;\n\n", name, value));
            }
        }

        tracker.into_parts()
    }

    // r[impl type.enum.ts.string]
    // r[impl type.enum.ts.explicit-string]
    // r[impl type.enum.ts.int]
    fn generate_enum(&self, enum_def: &EnumDef) -> String {
        let mut output = String::new();

        if let Some(ref doc) = enum_def.doc {
            output.push_str("/**\n");
            for line in doc.lines() {
                output.push_str(&format!(" * {}\n", line));
            }
            output.push_str(" */\n");
        }

        let name = to_pascal_case(&enum_def.name);

        match (&self.enum_style, enum_def.backing_type.as_str()) {
            (_, "integer") => {
                output.push_str(&format!("export enum {} {{\n", name));
                for variant in &enum_def.variants {
                    if let Value::Integer(i) = &variant.value {
                        output.push_str(&format!("  {} = {},\n", variant.name, i));
                    }
                }
                output.push_str("}\n\n");
            }
            (EnumStyle::Enum, "string") => {
                output.push_str(&format!("export enum {} {{\n", name));
                for variant in &enum_def.variants {
                    if let Value::String(s) = &variant.value {
                        output.push_str(&format!("  {} = \"{}\",\n", variant.name, s));
                    }
                }
                output.push_str("}\n\n");
            }
            (EnumStyle::Const, "string") => {
                output.push_str(&format!("export const {} = {{\n", name));
                for variant in &enum_def.variants {
                    if let Value::String(s) = &variant.value {
                        output.push_str(&format!("  {}: \"{}\",\n", variant.name, s));
                    }
                }
                output.push_str("} as const;\n");
                output.push_str(&format!(
                    "export type {} = (typeof {})[keyof typeof {}];\n\n",
                    name, name, name
                ));
            }
            (EnumStyle::Literal, "string") => {
                let values: Vec<_> = enum_def
                    .variants
                    .iter()
                    .filter_map(|v| {
                        if let Value::String(s) = &v.value {
                            Some(format!("\"{}\"", s))
                        } else {
                            None
                        }
                    })
                    .collect();

                output.push_str(&format!("export type {} = {};\n", name, values.join(" | ")));

                output.push_str(&format!("export const {} = {{\n", name));
                for variant in &enum_def.variants {
                    if let Value::String(s) = &variant.value {
                        output.push_str(&format!("  {}: \"{}\",\n", variant.name, s));
                    }
                }
                output.push_str("} as const;\n\n");
            }
            _ => {}
        }

        output
    }

    fn emit_alias(&self, tracker: &mut LineTracker, alias: &TypeAliasDef) {
        if let Some(ref doc) = alias.doc {
            tracker.push_str("/**\n");
            for line in doc.lines() {
                tracker.push_str(&format!(" * {}\n", line));
            }
            tracker.push_str(" */\n");
        }
        let target = self.generate_type(&alias.target);
        let name = to_pascal_case(&alias.name);
        tracker.add_mapping(format!("{}.{}", alias.namespace, alias.name), 13);
        tracker.push_str(&format!("export type {} = {};\n\n", name, target));
    }

    fn generate_index_file(&self, namespaces: &[String], prelude: Option<&str>) -> String {
        let mut output = String::new();

        if let Some(prelude) = prelude {
            output.push_str(prelude);
            if !prelude.ends_with('\n') {
                output.push('\n');
            }
        }

        output.push_str("// Generated by primate\n");
        output.push_str("// Do not edit manually.\n\n");

        for ns in namespaces {
            output.push_str(&format!("export * as {} from './{}';\n", ns, ns));
        }

        output
    }
}

impl Generator for TypeScriptGenerator {
    fn generate(&self, request: &CodeGenRequest) -> CodeGenResponse {
        let mut files = Vec::new();
        let prelude = request.options.get("prelude").and_then(|v| v.as_str());

        // Treat the configured `output` path as a directory unconditionally.
        // This is the only mode — there is no single-file output.
        let dir = request.output_path.trim_end_matches('/');

        let alias_map: HashMap<String, Type> = request
            .aliases
            .iter()
            .map(|a| (a.name.clone(), a.target.clone()))
            .collect();

        // Collect every namespace appearing across modules, enums, or aliases.
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
                path: format!("{}/{}.ts", dir, ns),
                content,
                mappings,
            });
        }

        let index_content = self.generate_index_file(&all_namespaces, prelude);
        files.push(GeneratedFile {
            path: format!("{}/index.ts", dir),
            content: index_content,
            mappings: vec![],
        });

        CodeGenResponse {
            files,
            errors: vec![],
        }
    }

    fn name(&self) -> &'static str {
        "typescript"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_integer() {
        let generator = TypeScriptGenerator::default();
        let value = generator.generate_value(&Value::Integer(42), &Type::I32);
        assert_eq!(value, "42");
    }

    #[test]
    fn test_generate_duration_number() {
        // r[verify type.duration.ts]
        let generator = TypeScriptGenerator::default();
        let value = generator.generate_value(
            &Value::Duration {
                nanoseconds: 30_000_000_000,
            },
            &Type::Duration,
        );
        assert_eq!(value, "30000");
    }

    #[test]
    fn test_generate_duration_temporal() {
        let mut generator = TypeScriptGenerator::default();
        generator.duration = DurationStyle::Temporal;
        let value = generator.generate_value(
            &Value::Duration {
                nanoseconds: 30_000_000_000,
            },
            &Type::Duration,
        );
        assert_eq!(value, "Temporal.Duration.from({ milliseconds: 30000 })");
    }

    #[test]
    fn test_convert_name_camel_case() {
        // r[verify naming.ts]
        let generator = TypeScriptGenerator::default();
        assert_eq!(generator.convert_name("MAX_RETRIES"), "maxRetries");
        assert_eq!(generator.convert_name("TIMEOUT"), "timeout");
    }

    #[test]
    fn test_convert_name_screaming_snake() {
        let mut generator = TypeScriptGenerator::default();
        generator.naming = NamingStyle::ScreamingSnakeCase;
        assert_eq!(generator.convert_name("MAX_RETRIES"), "MAX_RETRIES");
    }

    #[test]
    fn test_keyword_escape() {
        // r[verify naming.keyword-escape]
        let generator = TypeScriptGenerator::default();
        assert_eq!(generator.convert_name("TYPE"), "type_");
    }

    #[test]
    fn test_generate_array() {
        let generator = TypeScriptGenerator::default();
        let value = generator.generate_value(
            &Value::Array(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3),
            ]),
            &Type::Array {
                element: Box::new(Type::I32),
            },
        );
        assert_eq!(value, "[1, 2, 3]");
    }
}
