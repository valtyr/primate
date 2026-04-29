//! Intermediate Representation for primate
//!
//! Defines the IR structure that is shared between parsing and code generation.

use crate::types::{Type, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Source location information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    pub column: u32,
    #[serde(default)]
    pub length: Option<u32>,
}

/// A constant definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constant {
    /// Constant name (SCREAMING_SNAKE_CASE)
    pub name: String,

    /// Documentation comment
    pub doc: Option<String>,

    /// Type of the constant
    #[serde(rename = "type")]
    pub typ: Type,

    /// Normalized value
    pub value: Value,

    /// Source location
    pub source: SourceLocation,
}

/// An enum definition
// r[impl type.enum.simple]
// r[impl type.enum.string-backed]
// r[impl type.enum.int-backed]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDef {
    /// Enum name
    pub name: String,

    /// Namespace containing this enum
    pub namespace: String,

    /// Documentation comment
    pub doc: Option<String>,

    /// Enum variants
    pub variants: Vec<EnumVariant>,

    /// Backing type: "string" or "integer"
    #[serde(rename = "backingType")]
    pub backing_type: String,

    /// Source location
    pub source: SourceLocation,
}

/// An enum variant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    /// Variant name (PascalCase for generated code)
    pub name: String,

    /// Backing value (string or integer)
    pub value: Value,
}

/// A module (namespace) containing constants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    /// Namespace name
    pub namespace: String,

    /// Source file path
    #[serde(rename = "sourceFile")]
    pub source_file: String,

    /// Module-level documentation
    pub doc: Option<String>,

    /// Constants in this module
    pub constants: Vec<Constant>,
}

/// A type alias definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeAliasDef {
    /// Alias name (PascalCase)
    pub name: String,

    /// Namespace containing this alias
    pub namespace: String,

    /// Documentation comment
    pub doc: Option<String>,

    /// Underlying type the alias resolves to
    pub target: crate::types::Type,

    /// Source location
    pub source: SourceLocation,
}

/// Code generation request sent to plugins
// r[impl plugin.protocol.stdin]
// r[impl plugin.request.version]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenRequest {
    /// Protocol version
    pub version: u32,

    /// Output path from config
    // r[impl plugin.request.output-path]
    #[serde(rename = "outputPath")]
    pub output_path: String,

    /// Generator options from config
    // r[impl plugin.request.options]
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,

    /// Modules containing constants
    // r[impl plugin.request.modules]
    pub modules: Vec<Module>,

    /// Enum definitions
    // r[impl plugin.request.enums]
    pub enums: Vec<EnumDef>,

    /// Type alias definitions (emitted as standalone type declarations).
    #[serde(default)]
    pub aliases: Vec<TypeAliasDef>,
}

/// A generated file from a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedFile {
    /// Relative path for the file
    pub path: String,

    /// File content
    pub content: String,

    /// Symbol mappings for sourcemaps
    #[serde(default)]
    pub mappings: Vec<SymbolMapping>,
}

/// A mapping from a generated symbol to its source location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolMapping {
    /// Fully qualified symbol name
    pub symbol: String,

    /// Line in the generated file (1-based)
    pub line: u32,

    /// Column in the generated file (1-based)
    #[serde(default)]
    pub column: u32,
}

/// An error from a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginError {
    /// Error message
    pub message: String,

    /// Source location (optional)
    pub source: Option<SourceLocation>,
}

/// Code generation response from plugins
// r[impl plugin.protocol.stdout]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenResponse {
    /// Generated files
    // r[impl plugin.response.files]
    pub files: Vec<GeneratedFile>,

    /// Errors (optional)
    // r[impl plugin.response.errors]
    #[serde(default)]
    pub errors: Vec<PluginError>,
}

// r[impl pipeline.build-ir]
impl CodeGenRequest {
    /// Create a new code generation request
    pub fn new(output_path: String, options: HashMap<String, serde_json::Value>) -> Self {
        Self {
            version: 1,
            output_path,
            options,
            modules: Vec::new(),
            enums: Vec::new(),
            aliases: Vec::new(),
        }
    }
}
