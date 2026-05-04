//! `primate init` — scaffold a `primate.toml` in the current working
//! directory.
//!
//! Intentionally narrow: the only thing it produces is a commented
//! `primate.toml`. No `constants/` directory, no starter `.prim`,
//! no `.gitignore` munging, no AGENTS.md — primate gets dropped into
//! existing repos, never started in isolation.

use inquire::autocompletion::{Autocomplete, Replacement};
use inquire::{Confirm, CustomUserError, MultiSelect, Select, Text};
use std::path::{Path, PathBuf};

const CONFIG_PATH: &str = "primate.toml";

/// One built-in target the user picked, with their answers to the
/// associated style questions.
#[derive(Clone)]
struct BuiltinChoice {
    target: BuiltinTarget,
    path: String,
    /// The option the user explicitly picked. Other options render
    /// with their generator-default in the file so the user sees
    /// what's available to tune.
    chosen_option: (&'static str, String),
}

/// One external plugin entry.
#[derive(Clone)]
struct PluginChoice {
    name: String,
    command: String,
    path: String,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum BuiltinTarget {
    TypeScript,
    Rust,
    Python,
}

impl BuiltinTarget {
    fn id(self) -> &'static str {
        match self {
            BuiltinTarget::TypeScript => "typescript",
            BuiltinTarget::Rust => "rust",
            BuiltinTarget::Python => "python",
        }
    }
    fn pretty(self) -> &'static str {
        match self {
            BuiltinTarget::TypeScript => "TypeScript",
            BuiltinTarget::Rust => "Rust",
            BuiltinTarget::Python => "Python",
        }
    }
    fn default_path(self) -> &'static str {
        match self {
            BuiltinTarget::TypeScript => "web/src/generated/constants/",
            BuiltinTarget::Rust => "src/generated/constants.rs",
            BuiltinTarget::Python => "python/generated/constants/",
        }
    }
}

struct Answers {
    input_dir: String,
    builtins: Vec<BuiltinChoice>,
    plugins: Vec<PluginChoice>,
}

pub fn run(force: bool) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Path::new(CONFIG_PATH);
    if cfg.exists() && !force {
        return Err(format!(
            "{} already exists. Pass --force to overwrite, or delete it and \
             re-run.",
            CONFIG_PATH
        )
        .into());
    }

    println!();
    println!("  Setting up primate.\n");

    let answers = prompt()?;
    if answers.builtins.is_empty() && answers.plugins.is_empty() {
        return Err("No targets selected — nothing to generate.".into());
    }

    let toml = render_toml(&answers);
    std::fs::write(cfg, &toml).map_err(|e| format!("writing {}: {}", CONFIG_PATH, e))?;

    println!();
    println!("  ✓ Wrote {}", CONFIG_PATH);
    print_next_steps(&answers);
    Ok(())
}

// ────────────────────────────────────────────────────────────────────
// Prompts
// ────────────────────────────────────────────────────────────────────

fn prompt() -> Result<Answers, Box<dyn std::error::Error>> {
    let input_dir = Text::new("Where do your .prim source files live?")
        .with_default("constants")
        .with_help_message("Press → to accept the suggested directory")
        .with_autocomplete(PathAutocomplete::new())
        .prompt()?;

    let target_options = vec![
        BuiltinTarget::TypeScript,
        BuiltinTarget::Rust,
        BuiltinTarget::Python,
    ];
    let pretty: Vec<&'static str> = target_options.iter().map(|t| t.pretty()).collect();
    let picked_idx = MultiSelect::new("Which languages should primate generate?", pretty.clone())
        // No defaults — most projects don't ship to all three, so the
        // user picks intentionally rather than deselecting.
        .with_help_message("Space toggles · Enter confirms · Esc cancels")
        .raw_prompt()?
        .into_iter()
        .map(|opt| opt.index)
        .collect::<Vec<_>>();

    let mut builtins: Vec<BuiltinChoice> = Vec::new();
    for idx in picked_idx {
        let target = target_options[idx];
        let choice = configure_target(target)?;
        builtins.push(choice);
    }

    // External plugins: gated behind a yes/no so the prompt burst
    // only fires when wanted. Most projects don't need any.
    let mut plugins: Vec<PluginChoice> = Vec::new();
    let want_plugin = Confirm::new("Use an external generator (e.g. for Lua, Kotlin, …)?")
        .with_default(false)
        .with_help_message("Any executable that speaks primate's JSON plugin protocol.")
        .prompt()?;
    if want_plugin {
        loop {
            let name = Text::new("Plugin name")
                .with_help_message("Shows up in primate.toml — e.g. lua, kotlin, csharp")
                .prompt()?;
            let command = Text::new("Command")
                .with_help_message("Executable on PATH, or an absolute path")
                .with_autocomplete(PathAutocomplete::new())
                .prompt()?;
            let path = Text::new("Where should it write its output?")
                .with_autocomplete(PathAutocomplete::new())
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

/// Per-target sub-flow: ask the highest-impact style question first,
/// then where to write output. Other options surface as defaults
/// inside the generated file so the user sees what's available to
/// tune without paying for it in prompts.
fn configure_target(target: BuiltinTarget) -> Result<BuiltinChoice, Box<dyn std::error::Error>> {
    let chosen_option = match target {
        BuiltinTarget::TypeScript => {
            let pick = Select::new(
                "TypeScript: how should durations be represented?",
                vec![
                    "Milliseconds as number (default — works everywhere)",
                    "Temporal.Duration (richer; needs a Temporal polyfill)",
                ],
            )
            .with_help_message("Affects how `duration TIMEOUT = 30s` is emitted at the call site.")
            .raw_prompt()?;
            let value = if pick.index == 0 {
                "number"
            } else {
                "temporal"
            };
            ("duration", value.to_string())
        }
        BuiltinTarget::Rust => {
            let pick = Select::new(
                "Rust: how visible should the generated module be?",
                vec![
                    "pub (callers anywhere can use it)",
                    "pub(crate) (only the current crate)",
                    "pub(super) (only the parent module)",
                    "(private — only siblings of the file)",
                ],
            )
            .with_help_message("Sets the visibility on `pub mod`, `pub const`, `pub enum`, etc.")
            .raw_prompt()?;
            let value = match pick.index {
                0 => "pub",
                1 => "pub(crate)",
                2 => "pub(super)",
                _ => "",
            };
            ("visibility", value.to_string())
        }
        BuiltinTarget::Python => {
            let pick = Select::new(
                "Python: runtime values or .pyi-style type stubs?",
                vec![
                    "Runtime (regular .py with values you can import)",
                    "Stub (.pyi-style declarations only — for type-checking)",
                ],
            )
            .raw_prompt()?;
            let value = if pick.index == 0 { "runtime" } else { "stub" };
            ("typing", value.to_string())
        }
    };

    let path = Text::new(&format!("Where should the {} output go?", target.pretty()))
        .with_default(target.default_path())
        .with_help_message(target_path_help(target))
        .with_autocomplete(PathAutocomplete::new())
        .prompt()?;

    Ok(BuiltinChoice {
        target,
        path,
        chosen_option,
    })
}

fn target_path_help(target: BuiltinTarget) -> &'static str {
    match target {
        BuiltinTarget::TypeScript => {
            "A directory; primate writes one .ts per namespace plus an index.ts."
        }
        BuiltinTarget::Rust => "A single .rs file with one `pub mod` block per namespace.",
        BuiltinTarget::Python => {
            "A directory; primate writes one .py per namespace plus an __init__.py."
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// Path autocomplete
// ────────────────────────────────────────────────────────────────────

/// Suggests existing files and directories as the user types a path.
/// Directories are appended with `/` so a quick Tab keeps walking
/// down the tree.
#[derive(Clone, Default)]
struct PathAutocomplete {
    cache: Vec<String>,
    cached_for: String,
}

impl PathAutocomplete {
    fn new() -> Self {
        Self::default()
    }
}

impl Autocomplete for PathAutocomplete {
    fn get_suggestions(&mut self, input: &str) -> Result<Vec<String>, CustomUserError> {
        // Split the input into "directory to search" and "prefix to
        // match against entries in that directory". Trailing `/`
        // means the directory is the input verbatim and we list
        // everything inside it.
        let (dir, prefix) = split_path(input);
        if self.cached_for != dir.to_string_lossy() {
            self.cache = read_dir_sorted(&dir);
            self.cached_for = dir.to_string_lossy().into_owned();
        }

        let dir_str = dir.to_string_lossy();
        let prefix_str = prefix.to_string_lossy();
        let suggestions = self
            .cache
            .iter()
            .filter(|name| name.starts_with(prefix_str.as_ref()))
            .map(|name| join_for_display(&dir_str, name))
            .collect();
        Ok(suggestions)
    }

    fn get_completion(
        &mut self,
        _input: &str,
        highlighted_suggestion: Option<String>,
    ) -> Result<Replacement, CustomUserError> {
        Ok(match highlighted_suggestion {
            Some(s) => Replacement::Some(s),
            None => Replacement::None,
        })
    }
}

fn split_path(input: &str) -> (PathBuf, PathBuf) {
    if input.is_empty() {
        return (PathBuf::from("."), PathBuf::new());
    }
    if input.ends_with('/') {
        return (PathBuf::from(input.trim_end_matches('/')), PathBuf::new());
    }
    let path = Path::new(input);
    let dir = path.parent().unwrap_or(Path::new(""));
    let prefix = path.file_name().unwrap_or_default();
    let dir = if dir.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        dir.to_path_buf()
    };
    (dir, PathBuf::from(prefix))
}

fn read_dir_sorted(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let mut name = entry.file_name().to_string_lossy().into_owned();
            // Append `/` to directories so users see they can keep typing.
            if entry.path().is_dir() {
                name.push('/');
            }
            out.push(name);
        }
    }
    // Directories first, alphabetical inside each group.
    out.sort_by(|a, b| {
        let a_dir = a.ends_with('/');
        let b_dir = b.ends_with('/');
        b_dir.cmp(&a_dir).then_with(|| a.cmp(b))
    });
    out
}

fn join_for_display(dir: &str, name: &str) -> String {
    if dir == "." || dir.is_empty() {
        name.to_string()
    } else if dir.ends_with('/') {
        format!("{}{}", dir, name)
    } else {
        format!("{}/{}", dir, name)
    }
}

// ────────────────────────────────────────────────────────────────────
// TOML rendering
// ────────────────────────────────────────────────────────────────────

/// Build the `primate.toml` text. Comments are inlined per-section so
/// the file documents itself; every option a generator accepts gets
/// an entry with its default filled in (or the user's choice for the
/// option we asked about), so users discover what's tunable without
/// having to read the docs.
fn render_toml(a: &Answers) -> String {
    let mut s = String::new();
    s.push_str(
        "# primate.toml — generated by `primate init`.\n\
         #\n\
         # primate reads this file when you run `primate build`. It tells\n\
         # primate where your `.prim` source files live and which target\n\
         # languages to generate. Full reference:\n\
         # https://valtyr.github.io/primate/cli/build.html\n\
         #\n\
         # Tip: commit the generated files alongside source. Consumers\n\
         # don't need primate installed to use them, and CI can gate on\n\
         # `git diff --exit-code` after `primate build` to catch drift.\n\
         \n\
         # Where your `.prim` source files live. primate looks for\n\
         # `*.prim` files inside this directory and any subdirectory;\n\
         # subdirectories become `::`-separated namespaces in generated\n\
         # output (so `auth/sessions.prim` ends up in `auth::sessions`).\n",
    );
    s.push_str(&format!("input = {}\n\n", quote_toml(&a.input_dir)));

    for b in &a.builtins {
        s.push_str("[[output]]\n");
        s.push_str(&format!("generator = \"{}\"\n", b.target.id()));
        match b.target {
            BuiltinTarget::TypeScript => s.push_str(
                "# A directory. primate emits one `.ts` per namespace plus\n\
                 # an `index.ts` that re-exports each one.\n",
            ),
            BuiltinTarget::Rust => s.push_str(
                "# A single `.rs` file with one `pub mod <ns>` block per\n\
                 # namespace. Cross-namespace references become\n\
                 # `super::other::X`.\n",
            ),
            BuiltinTarget::Python => s.push_str(
                "# A package directory. primate emits one `.py` per\n\
                 # namespace plus an `__init__.py` that re-exports each.\n",
            ),
        }
        s.push_str(&format!("path      = {}\n", quote_toml(&b.path)));

        s.push('\n');
        s.push_str("# Generator options. Defaults shown — change as needed.\n");
        for (key, default, comment) in target_options(b.target) {
            let value = if b.chosen_option.0 == *key {
                b.chosen_option.1.as_str()
            } else {
                *default
            };
            // Pad the key column so values align across options.
            s.push_str(&format!(
                "options.{:11} = {:<25} # {}\n",
                key,
                quote_toml(value),
                comment
            ));
        }
        s.push('\n');
    }

    for p in &a.plugins {
        s.push_str(
            "[[output]]\n\
             # External plugin — any executable that reads JSON on stdin\n\
             # and writes JSON on stdout (see the plugin protocol docs).\n",
        );
        s.push_str(&format!("plugin  = {}\n", quote_toml(&p.name)));
        s.push_str(&format!("command = {}\n", quote_toml(&p.command)));
        s.push_str(&format!("path    = {}\n\n", quote_toml(&p.path)));
    }

    s
}

/// Every (key, default, one-line description) for a built-in target's
/// options. `init` writes them all into the file so users can see
/// what's tunable without having to look it up.
fn target_options(target: BuiltinTarget) -> &'static [(&'static str, &'static str, &'static str)] {
    match target {
        BuiltinTarget::TypeScript => &[
            ("naming", "camelCase", "or \"SCREAMING_SNAKE_CASE\""),
            (
                "duration",
                "number",
                "or \"temporal\" for Temporal.Duration",
            ),
            ("u64", "number", "or \"bigint\" for u64-typed constants"),
            (
                "enumStyle",
                "literal",
                "or \"const\" or \"enum\" — see docs",
            ),
        ],
        BuiltinTarget::Rust => &[(
            "visibility",
            "pub",
            "or \"pub(crate)\", \"pub(super)\", \"\"",
        )],
        BuiltinTarget::Python => &[("typing", "runtime", "or \"stub\" for a .pyi-style file")],
    }
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
    println!("  Next:");
    println!(
        "    mkdir -p {} && touch {}/example.prim",
        a.input_dir, a.input_dir
    );
    println!(
        "    Edit {}/example.prim — e.g. `u32 ANSWER = 42`",
        a.input_dir
    );
    println!("    primate build");
    println!();
    println!("  Tip: commit the generated output alongside source so consumers");
    println!("  don't need primate installed and CI catches drift via");
    println!("  `git diff --exit-code` after `primate build`.");
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts_choice(path: &str, duration: &str) -> BuiltinChoice {
        BuiltinChoice {
            target: BuiltinTarget::TypeScript,
            path: path.into(),
            chosen_option: ("duration", duration.into()),
        }
    }

    #[test]
    fn renders_picked_option() {
        let a = Answers {
            input_dir: "constants".into(),
            builtins: vec![ts_choice("web/src/generated/constants/", "temporal")],
            plugins: Vec::new(),
        };
        let out = render_toml(&a);
        // The user's pick lands in the toml.
        assert!(
            out.contains("options.duration    = \"temporal\""),
            "got:\n{}",
            out
        );
        // Other options come along with their generator defaults so
        // the user discovers what's tunable.
        assert!(out.contains("options.naming      = \"camelCase\""));
        assert!(out.contains("options.u64         = \"number\""));
        assert!(out.contains("options.enumStyle   = \"literal\""));
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
    }

    #[test]
    fn quote_handles_special_chars() {
        assert_eq!(quote_toml("hello"), "\"hello\"");
        assert_eq!(quote_toml(r#"with"quote"#), r#""with\"quote""#);
        assert_eq!(quote_toml("with\\back"), r#""with\\back""#);
    }

    #[test]
    fn split_path_examples() {
        assert_eq!(split_path(""), (PathBuf::from("."), PathBuf::from("")));
        assert_eq!(
            split_path("src/"),
            (PathBuf::from("src"), PathBuf::from(""))
        );
        assert_eq!(
            split_path("src/gen"),
            (PathBuf::from("src"), PathBuf::from("gen"))
        );
        assert_eq!(
            split_path("README"),
            (PathBuf::from("."), PathBuf::from("README"))
        );
    }
}
