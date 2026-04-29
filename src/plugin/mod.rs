//! Plugin system for external code generators
//!
//! Plugins are standalone executables that receive IR on stdin and write
//! generated files to stdout.

use crate::ir::{CodeGenRequest, CodeGenResponse};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Resolve a plugin name or path to an executable path
// r[impl plugin.resolve.path]
// r[impl plugin.resolve.name]
// r[impl plugin.resolve.fallback]
pub fn resolve_plugin(name_or_path: &str) -> Result<PathBuf, PluginError> {
    // If it's a path, use directly
    if name_or_path.starts_with('.') || name_or_path.starts_with('/') {
        let path = PathBuf::from(name_or_path);
        if path.exists() {
            return Ok(path);
        }
        return Err(PluginError::NotFound(name_or_path.to_string()));
    }

    // Look for primate-gen-<name> in PATH
    let exe_name = format!("primate-gen-{}", name_or_path);
    if let Ok(path) = which::which(&exe_name) {
        return Ok(path);
    }

    // Fallback to ~/.primate/plugins/<name>
    if let Some(home) = dirs::home_dir() {
        let fallback = home.join(".primate/plugins").join(name_or_path);
        if fallback.exists() {
            return Ok(fallback);
        }
    }

    Err(PluginError::NotFound(name_or_path.to_string()))
}

/// Invoke a plugin with the given request
// r[impl plugin.executable]
pub fn invoke_plugin(
    plugin_path: &Path,
    request: &CodeGenRequest,
) -> Result<CodeGenResponse, PluginError> {
    let request_json = serde_json::to_string(request)?;

    let mut child = Command::new(plugin_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| PluginError::SpawnFailed(plugin_path.to_path_buf(), e))?;

    // Write request to stdin
    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(request_json.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    // r[impl plugin.exit.success]
    // r[impl plugin.exit.failure]
    // r[impl plugin.exit.crash]
    match output.status.code() {
        Some(0) => {
            let response: CodeGenResponse = serde_json::from_slice(&output.stdout)?;
            Ok(response)
        }
        Some(1) => {
            // Try to parse error response
            if let Ok(response) = serde_json::from_slice::<CodeGenResponse>(&output.stdout) {
                Ok(response)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(PluginError::Failed(stderr.to_string()))
            }
        }
        Some(code) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(PluginError::Crashed(code, stderr.to_string()))
        }
        None => Err(PluginError::Crashed(
            -1,
            "Plugin terminated by signal".to_string(),
        )),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin not found: {0}")]
    NotFound(String),

    #[error("failed to spawn plugin {0}: {1}")]
    SpawnFailed(PathBuf, std::io::Error),

    #[error("plugin failed: {0}")]
    Failed(String),

    #[error("plugin crashed with exit code {0}: {1}")]
    Crashed(i32, String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
