//! Configuration handling for primate.toml
//!
//! Parses and validates the project configuration file.

use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Stable URL for the JSON Schema describing `primate.toml`. Editors
/// like taplo and Even Better TOML follow `#:schema <URL>` comments
/// at the top of TOML files for autocompletion + validation; the
/// schema itself is derived from the [`Config`] struct in this file
/// via the `gen-schema` bin and committed at the repo root.
pub const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/valtyr/primate/main/primate.schema.json";

/// Root configuration structure.
// r[impl config.file]
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(title = "primate.toml", deny_unknown_fields)]
pub struct Config {
    /// Directory of `.prim` source files, relative to `primate.toml`.
    /// primate walks this recursively; subdirectories become
    /// `::`-separated namespaces in generated output.
    // r[impl config.input.required]
    pub input: PathBuf,

    /// Where the IDE sourcemap is written (defaults to
    /// `primate.sourcemap.json` next to this config). The LSP uses
    /// it to jump between source `.prim` lines and generated output.
    pub sourcemap: Option<PathBuf>,

    /// One [[output]] block per generator target. At least one is
    /// required.
    // r[impl config.output.required]
    #[serde(rename = "output")]
    pub outputs: Vec<OutputConfig>,
}

/// One generator target.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct OutputConfig {
    /// Built-in generator name. Exactly one of `generator` or
    /// `plugin` must be set.
    // r[impl config.generator.builtin]
    pub generator: Option<String>,

    /// External plugin name (an executable on `PATH`, or an absolute
    /// path). Exactly one of `generator` or `plugin` must be set.
    pub plugin: Option<String>,

    /// Where the generator writes its output. A directory for
    /// `typescript` and `python`; a single `.rs` file for `rust`.
    // r[impl config.output.path]
    pub path: PathBuf,

    /// Generator-specific knobs. The accepted keys depend on the
    /// generator — see the per-generator section in the build docs.
    // r[impl config.output.options]
    #[serde(default)]
    #[schemars(with = "std::collections::HashMap<String, serde_json::Value>")]
    pub options: HashMap<String, toml::Value>,
}

impl Config {
    /// Load configuration from a file
    pub fn load(path: &std::path::Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Get the sourcemap path, using the override if specified, otherwise defaulting
    /// to primate.sourcemap.json in the same directory as the config file.
    pub fn sourcemap_path(&self, config_path: &std::path::Path) -> PathBuf {
        // Get the config directory, handling empty parent paths
        let config_dir = config_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));

        if let Some(ref sourcemap) = self.sourcemap {
            // If sourcemap path is relative, resolve it relative to config directory
            if sourcemap.is_absolute() {
                sourcemap.clone()
            } else {
                config_dir.join(sourcemap)
            }
        } else {
            // Default: primate.sourcemap.json next to the config file
            config_dir.join("primate.sourcemap.json")
        }
    }

    /// Validate the configuration
    // r[impl config.output.generator-or-plugin]
    fn validate(&self) -> Result<(), ConfigError> {
        if self.outputs.is_empty() {
            return Err(ConfigError::NoOutputs);
        }

        for (i, output) in self.outputs.iter().enumerate() {
            match (&output.generator, &output.plugin) {
                (Some(_), Some(_)) => {
                    return Err(ConfigError::BothGeneratorAndPlugin(i));
                }
                (None, None) => {
                    return Err(ConfigError::NeitherGeneratorNorPlugin(i));
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Check configuration content and return diagnostics
    pub fn check(content: &str, file_path: &str) -> crate::diagnostics::Diagnostics {
        use crate::diagnostics::{Diagnostic, Diagnostics, Severity};

        let mut diagnostics = Diagnostics::new();

        match toml::from_str::<Config>(content) {
            Ok(config) => {
                // Run logical validation
                if let Err(e) = config.validate() {
                    // Logic errors often don't have line numbers attached easily,
                    // unless we use toml::Document to find them.
                    // For now, we'll map them to line 1 or try to find the output section.
                    // Improving this would require parsing as toml::Document first to find spans.

                    let (message, index) = match &e {
                        ConfigError::NoOutputs => {
                            ("at least one [[output]] is required".to_string(), None)
                        }
                        ConfigError::BothGeneratorAndPlugin(i) => (
                            format!(
                                "output[{}]: exactly one of 'generator' or 'plugin' must be specified, but both were",
                                i
                            ),
                            Some(*i),
                        ),
                        ConfigError::NeitherGeneratorNorPlugin(i) => (
                            format!(
                                "output[{}]: exactly one of 'generator' or 'plugin' must be specified, but neither was",
                                i
                            ),
                            Some(*i),
                        ),
                        _ => (e.to_string(), None),
                    };

                    let line = if let Some(idx) = index {
                        // Try to find the line number for [[output]] #idx
                        find_output_line(content, idx).unwrap_or(1)
                    } else {
                        1
                    };

                    diagnostics.add(Diagnostic {
                        file: file_path.to_string(),
                        line,
                        column: 1,
                        length: None,
                        severity: Severity::Error,
                        code: "config-error".to_string(),
                        message,
                        targets: vec![],
                    });
                }
            }
            Err(e) => {
                let (line, col) = if let Some(span) = e.span() {
                    // Calculate line/col from span
                    let (l, c) = index_to_line_col(content, span.start);
                    (l as u32, c as u32)
                } else {
                    (1, 1)
                };

                diagnostics.add(Diagnostic {
                    file: file_path.to_string(),
                    line,
                    column: col,
                    length: None,
                    severity: Severity::Error,
                    code: "parse-error".to_string(),
                    message: e.to_string(),
                    targets: vec![],
                });
            }
        }

        diagnostics
    }
}

fn index_to_line_col(content: &str, index: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, c) in content.char_indices() {
        if i == index {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn find_output_line(content: &str, index: usize) -> Option<u32> {
    let mut count = 0;
    for (i, line) in content.lines().enumerate() {
        if line.trim().starts_with("[[output]]") {
            if count == index {
                return Some((i + 1) as u32);
            }
            count += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::Config;

    /// Belt-and-braces: if anyone changes the `Config` struct
    /// without re-running `cargo run --bin gen-schema`, the
    /// committed schema and the runtime-generated schema will
    /// disagree and this test will fail loudly. CI runs the bin
    /// itself and `git diff --exit-code`s; this test is what
    /// developers see locally.
    #[test]
    fn committed_schema_matches_generated() {
        let generated = schemars::schema_for!(Config);
        let mut generated_json = serde_json::to_string_pretty(&generated).unwrap();
        generated_json.push('\n');

        let committed =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/primate.schema.json"))
                .expect("primate.schema.json missing — run `cargo run --bin gen-schema`");

        assert_eq!(
            committed.trim_end(),
            generated_json.trim_end(),
            "primate.schema.json is stale — run `cargo run --bin gen-schema` and commit"
        );
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("at least one [[output]] is required")]
    NoOutputs,

    #[error("output[{0}]: exactly one of 'generator' or 'plugin' must be specified, but both were")]
    BothGeneratorAndPlugin(usize),

    #[error(
        "output[{0}]: exactly one of 'generator' or 'plugin' must be specified, but neither was"
    )]
    NeitherGeneratorNorPlugin(usize),
}
