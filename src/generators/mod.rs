//! Built-in code generators
//!
//! Implements code generation for TypeScript, Rust, and Python.

pub mod python;
pub mod rust;
pub mod typescript;

use crate::ir::{CodeGenRequest, CodeGenResponse};

/// Trait for built-in generators
pub trait Generator {
    /// Generate code from the IR
    fn generate(&self, request: &CodeGenRequest) -> CodeGenResponse;

    /// Generator name
    fn name(&self) -> &'static str;
}

/// Get a built-in generator by name
// r[impl config.generator.builtin]
pub fn get_generator(name: &str) -> Option<Box<dyn Generator>> {
    match name {
        "typescript" => Some(Box::new(typescript::TypeScriptGenerator::default())),
        "rust" => Some(Box::new(rust::RustGenerator::default())),
        "python" => Some(Box::new(python::PythonGenerator::default())),
        _ => None,
    }
}
