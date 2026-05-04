use zed_extension_api::{self as zed, Command, LanguageServerId, Result, Worktree};

/// Zed extension that launches `primate lsp`. Wires up the LSP for
/// `.prim` files (full primate language support) and for
/// `typescript`/`javascript`/`rust`/`python` files (so go-to-definition
/// on a generated constant jumps back to its `.prim` source via the
/// sourcemap).
///
/// We gate the LSP launch on the worktree containing a `primate.toml`
/// at its root. Without that gate, the extension would spin up a
/// `primate` process for every TS/Rust/Python project the user opens
/// even when there's no primate involved — wasteful, and it can
/// surface a "primate not found" error in projects that have nothing
/// to do with primate.
///
/// The `primate` binary is resolved via `which` on the worktree's
/// PATH; if you want to point at a custom build, expose it on PATH
/// (or symlink the dev binary into `~/.cargo/bin`).
struct PrimateExtension;

impl zed::Extension for PrimateExtension {
    fn new() -> Self {
        PrimateExtension
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        // Only run for actual primate projects — the heuristic is a
        // `primate.toml` at the worktree root. Returning an error
        // here keeps the LSP from starting at all in unrelated
        // projects, so other LSPs aren't disturbed.
        if worktree.read_text_file("primate.toml").is_err() {
            return Err(
                "no primate.toml at worktree root — skipping primate LSP".to_string(),
            );
        }

        let binary = worktree.which("primate").ok_or_else(|| {
            "could not find `primate` on PATH. Install with `cargo install primate \
             --locked`, or symlink a dev build into `~/.cargo/bin`."
                .to_string()
        })?;

        Ok(Command {
            command: binary,
            args: vec!["lsp".to_string()],
            env: vec![],
        })
    }
}

zed::register_extension!(PrimateExtension);
