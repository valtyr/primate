//! `primate init` — scaffold a `primate.toml` in the current working
//! directory.
//!
//! Intentionally narrow: the only thing it produces is a commented
//! `primate.toml`. No `constants/` directory, no starter `.prim`,
//! no `.gitignore` munging, no AGENTS.md — primate gets dropped into
//! existing repos, never started in isolation.
//!
//! Prompt-driven by default; `--yes` accepts every default for
//! scripted use; `--force` overwrites an existing config.

use inquire::{Confirm, MultiSelect, Text};
use std::path::Path;

const CONFIG_PATH: &str = "primate.toml";

/// One built-in target the user picked, with the path they want it
/// written to.
struct BuiltinChoice {
    name: &'static str,
    path: String,
}

/// One external plugin entry. The `command` is the executable name
/// (resolved on `PATH`) or absolute path.
struct PluginChoice {
    name: String,
    command: String,
    path: String,
}

struct Answers {
    input_dir: String,
    builtins: Vec<BuiltinChoice>,
    plugins: Vec<PluginChoice>,
}

pub fn run(yes: bool, force: bool) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Path::new(CONFIG_PATH);
    if cfg.exists() && !force {
        return Err(format!(
            "{} already exists. Pass --force to overwrite, or delete it and \
             re-run.",
            CONFIG_PATH
        )
        .into());
    }

    let answers = if yes { defaults() } else { prompt()? };

    if answers.builtins.is_empty() && answers.plugins.is_empty() {
        return Err("no targets selected — nothing to generate".into());
    }

    let toml = render_toml(&answers);
    std::fs::write(cfg, &toml).map_err(|e| format!("writing {}: {}", CONFIG_PATH, e))?;

    println!();
    println!("✓ wrote {}", CONFIG_PATH);
    print_next_steps(&answers);
    Ok(())
}

/// All-defaults answers used by `--yes`. Picks all three built-in
/// targets at sensible paths and skips external plugins.
fn defaults() -> Answers {
    Answers {
        input_dir: "constants".into(),
        builtins: vec![
            BuiltinChoice {
                name: "typescript",
                path: default_path("typescript").into(),
            },
            BuiltinChoice {
                name: "rust",
                path: default_path("rust").into(),
            },
            BuiltinChoice {
                name: "python",
                path: default_path("python").into(),
            },
        ],
        plugins: Vec::new(),
    }
}

fn default_path(name: &str) -> &'static str {
    match name {
        "typescript" => "web/src/generated/constants/",
        "rust" => "src/generated/constants.rs",
        "python" => "python/generated/constants/",
        _ => "",
    }
}

fn prompt() -> Result<Answers, Box<dyn std::error::Error>> {
    let input_dir = Text::new("Source directory for .prim files")
        .with_default("constants")
        .with_help_message(
            "primate walks this directory recursively; namespaces follow \
             the layout.",
        )
        .prompt()?;

    let target_names = vec!["typescript", "rust", "python"];
    let picked = MultiSelect::new("Targets to generate", target_names.clone())
        // Default: everything selected. Most projects ship to at least
        // two of these, and removing one is faster than enabling one.
        .with_default(&[0, 1, 2])
        .with_help_message("Space toggles · Enter confirms")
        .prompt()?;

    let mut builtins: Vec<BuiltinChoice> = Vec::new();
    for name in &picked {
        let path = Text::new(&format!("{} output path", name))
            .with_default(default_path(name))
            .with_help_message(path_help_for(name))
            .prompt()?;
        builtins.push(BuiltinChoice { name, path });
    }

    // External plugins: ask once whether the user wants any, then
    // loop. Most projects don't, so the question gates the prompt
    // burst rather than the prompt itself.
    let mut plugins: Vec<PluginChoice> = Vec::new();
    let want_plugin = Confirm::new("Add an external generator plugin?")
        .with_default(false)
        .with_help_message(
            "Plugins are any executable that reads JSON on stdin and \
             writes JSON on stdout. See docs/plugins.",
        )
        .prompt()?;
    if want_plugin {
        loop {
            let name = Text::new("Plugin name")
                .with_help_message("e.g. lua, kotlin, csharp")
                .prompt()?;
            let command = Text::new("Command")
                .with_help_message("Executable on PATH or absolute path")
                .prompt()?;
            let path = Text::new("Output path")
                .with_help_message("File or directory — your plugin decides")
                .prompt()?;
            plugins.push(PluginChoice {
                name,
                command,
                path,
            });
            let more = Confirm::new("Add another plugin?")
                .with_default(false)
                .prompt()?;
            if !more {
                break;
            }
        }
    }

    Ok(Answers {
        input_dir,
        builtins,
        plugins,
    })
}

fn path_help_for(name: &str) -> &'static str {
    match name {
        "typescript" => "Directory; primate emits one .ts per namespace + index.ts.",
        "rust" => "Single .rs file with pub mod blocks per namespace.",
        "python" => "Directory; one .py per namespace + __init__.py.",
        _ => "",
    }
}

/// Build the `primate.toml` text. Comments are inlined per-section so
/// the file documents itself; only the targets the user actually
/// picked are emitted (no commented-out scaffolding for skipped ones).
fn render_toml(a: &Answers) -> String {
    let mut s = String::new();
    s.push_str(
        "# primate.toml — generated by `primate init`.\n\
         #\n\
         # Tells `primate build` where to find your `.prim` source files\n\
         # and which target languages to generate. Keep this file at the\n\
         # project root. Full reference:\n\
         # https://valtyr.github.io/primate/cli/build.html\n\
         #\n\
         # Tip: commit the generated files alongside source. Consumers\n\
         # don't need primate installed to use them, and CI can gate on\n\
         # `git diff --exit-code` after `primate build` to catch drift.\n\
         \n\
         # Directory of `.prim` files. primate walks this recursively;\n\
         # each file's namespace defaults to its path relative to this\n\
         # directory.\n",
    );
    s.push_str(&format!("input = {}\n\n", quote_toml(&a.input_dir)));

    for b in &a.builtins {
        s.push_str("[[output]]\n");
        s.push_str(&format!("generator = \"{}\"\n", b.name));
        match b.name {
            "typescript" => s.push_str(
                "# Directory — primate emits one .ts per namespace plus an\n\
                 # index.ts that re-exports each one.\n",
            ),
            "rust" => s.push_str(
                "# Single .rs file with one `pub mod <ns>` block per\n\
                 # namespace. Cross-namespace refs become `super::other::X`.\n",
            ),
            "python" => s.push_str(
                "# Package directory — one .py per namespace plus an\n\
                 # __init__.py that re-exports each as a submodule.\n",
            ),
            _ => {}
        }
        s.push_str(&format!("path      = {}\n", quote_toml(&b.path)));
        if b.name == "typescript" {
            s.push_str(
                "# Optional knobs:\n\
                 # options.naming   = \"camelCase\"          # or \"SCREAMING_SNAKE_CASE\"\n\
                 # options.duration = \"number\"             # or \"temporal\"\n\
                 # options.u64      = \"number\"             # or \"bigint\"\n\
                 # options.enumStyle = \"literal\"           # or \"const\", \"enum\"\n",
            );
        }
        s.push('\n');
    }

    for p in &a.plugins {
        s.push_str("[[output]]\n");
        s.push_str("# External plugin — reads JSON on stdin, writes JSON on\n");
        s.push_str("# stdout. See docs/plugins for the protocol.\n");
        s.push_str(&format!("plugin  = {}\n", quote_toml(&p.name)));
        s.push_str(&format!("command = {}\n", quote_toml(&p.command)));
        s.push_str(&format!("path    = {}\n\n", quote_toml(&p.path)));
    }

    s
}

/// Encode a string as a TOML basic string. Conservatively escapes
/// only what the TOML spec requires.
fn quote_toml(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn print_next_steps(a: &Answers) {
    println!();
    println!("next:");
    println!(
        "  mkdir -p {} && touch {}/example.prim",
        a.input_dir, a.input_dir
    );
    println!(
        "  edit {}/example.prim — e.g. `u32 ANSWER = 42`",
        a.input_dir
    );
    println!("  primate build");
    println!();
    println!(
        "tip: commit the generated files alongside source. Consumers won't\n     \
         need primate installed, and CI can catch drift with\n     \
         `git diff --exit-code` after `primate build`."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_minimal() {
        let a = Answers {
            input_dir: "constants".into(),
            builtins: vec![BuiltinChoice {
                name: "rust",
                path: "src/generated/constants.rs".into(),
            }],
            plugins: Vec::new(),
        };
        let out = render_toml(&a);
        assert!(out.starts_with("# primate.toml"));
        assert!(out.contains("input = \"constants\""));
        assert!(out.contains("generator = \"rust\""));
        assert!(out.contains("path      = \"src/generated/constants.rs\""));
    }

    #[test]
    fn renders_plugin() {
        let a = Answers {
            input_dir: "constants".into(),
            builtins: Vec::new(),
            plugins: vec![PluginChoice {
                name: "lua".into(),
                command: "primate-lua".into(),
                path: "scripts/generated/constants.lua".into(),
            }],
        };
        let out = render_toml(&a);
        assert!(out.contains("plugin  = \"lua\""));
        assert!(out.contains("command = \"primate-lua\""));
        assert!(out.contains("path    = \"scripts/generated/constants.lua\""));
    }

    #[test]
    fn quote_handles_special_chars() {
        assert_eq!(quote_toml("hello"), "\"hello\"");
        assert_eq!(quote_toml(r#"with"quote"#), r#""with\"quote""#);
        assert_eq!(quote_toml("with\\back"), r#""with\\back""#);
    }
}
