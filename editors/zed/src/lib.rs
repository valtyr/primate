use zed_extension_api::{self as zed, Command, LanguageServerId, Result, Worktree};

/// Zed extension that launches `primate lsp` for `.prim` files.
///
/// The `primate` binary is resolved via `which` on the worktree's PATH.
/// If you want to point at a custom build, expose it on PATH (or symlink it
/// into `~/.cargo/bin`).
struct CconsttExtension;

impl zed::Extension for CconsttExtension {
    fn new() -> Self {
        CconsttExtension
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        let binary = worktree.which("primate").ok_or_else(|| {
            "could not find `primate` on PATH. Install it from the project root with \
             `cargo install --path .` or symlink the dev binary into `~/.cargo/bin`."
                .to_string()
        })?;

        Ok(Command {
            command: binary,
            args: vec!["lsp".to_string()],
            env: vec![],
        })
    }
}

zed::register_extension!(CconsttExtension);
