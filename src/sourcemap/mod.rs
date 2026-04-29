//! Source mapping for IDE navigation
//!
//! Provides mapping from generated code back to .c.toml sources.

use serde::{Deserialize, Serialize};

/// Sourcemap entry linking a symbol to its source location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcemapEntry {
    /// Fully qualified symbol name (e.g., "time.OFFLINE_THRESHOLD")
    pub symbol: String,

    /// Source .c.toml file
    #[serde(rename = "sourceFile")]
    pub source_file: String,

    /// Line in source file
    #[serde(rename = "sourceLine")]
    pub source_line: u32,

    /// Column in source file
    #[serde(rename = "sourceColumn")]
    pub source_column: u32,

    /// Generated output file
    #[serde(rename = "outputFile")]
    pub output_file: String,

    /// Line in output file
    #[serde(rename = "outputLine")]
    pub output_line: u32,

    /// Column in output file (1-based)
    #[serde(rename = "outputColumn", default)]
    pub output_column: u32,
}

/// Complete sourcemap for a project
// r[impl sourcemap.json]
// r[impl sourcemap.format]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sourcemap {
    /// Sourcemap format version
    pub version: u32,

    /// All sourcemap entries
    pub entries: Vec<SourcemapEntry>,
}

impl Sourcemap {
    /// Create a new empty sourcemap
    pub fn new() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }

    /// Add an entry to the sourcemap
    pub fn add_entry(&mut self, entry: SourcemapEntry) {
        self.entries.push(entry);
    }

    /// Write sourcemap to JSON
    // r[impl pipeline.sourcemap]
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

impl Default for Sourcemap {
    fn default() -> Self {
        Self::new()
    }
}
