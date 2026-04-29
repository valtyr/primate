//! Diagnostic reporting for errors, warnings, and info messages
//!
//! Target-aware diagnostics system that considers configured outputs.

use crate::ir::SourceLocation;
use serde::{Deserialize, Serialize};

/// Diagnostic severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A diagnostic message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Source file path
    pub file: String,

    /// Line number (1-indexed)
    pub line: u32,

    /// Column number (1-indexed)
    pub column: u32,

    /// Length of the range (in characters)
    #[serde(default)]
    pub length: Option<u32>,

    /// Severity level
    pub severity: Severity,

    /// Diagnostic code (e.g., "js-unsafe-integer")
    pub code: String,

    /// Human-readable message
    pub message: String,

    /// Target languages this diagnostic applies to (empty = all)
    // r[impl diag.warn.target-aware]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
}

impl Diagnostic {
    /// Create a new error diagnostic
    pub fn error(source: &SourceLocation, code: &str, message: String) -> Self {
        Self {
            file: source.file.clone(),
            line: source.line,
            column: source.column,
            length: source.length,
            severity: Severity::Error,
            code: code.to_string(),
            message,
            targets: vec![],
        }
    }

    /// Create a new warning diagnostic
    pub fn warning(source: &SourceLocation, code: &str, message: String) -> Self {
        Self {
            file: source.file.clone(),
            line: source.line,
            column: source.column,
            length: source.length,
            severity: Severity::Warning,
            code: code.to_string(),
            message,
            targets: vec![],
        }
    }

    /// Create a new info diagnostic
    pub fn info(source: &SourceLocation, code: &str, message: String) -> Self {
        Self {
            file: source.file.clone(),
            line: source.line,
            column: source.column,
            length: source.length,
            severity: Severity::Info,
            code: code.to_string(),
            message,
            targets: vec![],
        }
    }

    /// Set target languages for this diagnostic
    pub fn with_targets(mut self, targets: Vec<String>) -> Self {
        self.targets = targets;
        self
    }
}

/// Collection of diagnostics
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Diagnostics {
    pub diagnostics: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Create new empty diagnostics
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a diagnostic
    pub fn add(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Check if there are any errors
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Filter diagnostics by target
    pub fn for_target(&self, target: &str) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.targets.is_empty() || d.targets.contains(&target.to_string()))
            .collect()
    }
}

// Diagnostic code constants

// Errors
// r[impl diag.error.unknown-type]
pub const ERR_UNKNOWN_TYPE: &str = "unknown-type";
// r[impl diag.error.parse-failure]
pub const ERR_PARSE_FAILURE: &str = "parse-failure";
// r[impl diag.error.duplicate-name]
pub const ERR_DUPLICATE_NAME: &str = "duplicate-name";
// r[impl diag.error.invalid-identifier]
pub const ERR_INVALID_IDENTIFIER: &str = "invalid-identifier";
// r[impl diag.error.type-mismatch]
pub const ERR_TYPE_MISMATCH: &str = "type-mismatch";
// r[impl diag.error.naming-convention]
pub const ERR_NAMING_CONVENTION: &str = "naming-convention";
// r[impl diag.error.enum-variant]
pub const ERR_INVALID_ENUM_VARIANT: &str = "invalid-enum-variant";
// r[impl diag.error.overflow]
pub const ERR_OVERFLOW: &str = "overflow";
// r[impl diag.error.byte-overflow]
pub const ERR_BYTE_OVERFLOW: &str = "byte-overflow";
// r[impl diag.error.circular-namespace]
pub const ERR_CIRCULAR_NAMESPACE: &str = "circular-namespace";

// Warnings
// r[impl diag.warn.unsafe-integer]
pub const WARN_UNSAFE_INTEGER: &str = "js-unsafe-integer";
// r[impl diag.warn.duration-precision]
pub const WARN_DURATION_PRECISION: &str = "duration-precision";
// r[impl diag.warn.keyword-collision]
pub const WARN_KEYWORD_COLLISION: &str = "keyword-collision";
// r[impl diag.warn.unused-namespace]
pub const WARN_UNUSED_NAMESPACE: &str = "unused-namespace";

// Info
// r[impl diag.info.deprecated]
pub const INFO_DEPRECATED: &str = "deprecated";
// r[impl diag.info.style]
pub const INFO_STYLE: &str = "style";
