//! `primate init` — scaffold a `primate.toml` in the current working
//! directory.
//!
//! Intentionally narrow: the only thing it produces is a commented
//! `primate.toml`. No `constants/` directory, no starter `.prim`,
//! no `.gitignore` munging, no AGENTS.md — primate gets dropped into
//! existing repos, never started in isolation.

use inquire::autocompletion::{Autocomplete, Replacement};
use inquire::ui::{Color, RenderConfig, StyleSheet, Styled};
use inquire::validator::ValueRequiredValidator;
use inquire::{Confirm, CustomUserError, MultiSelect, Text};
use std::path::{Path, PathBuf};

const CONFIG_PATH: &str = "primate.toml";

/// One built-in target the user picked.
#[derive(Clone)]
struct BuiltinChoice {
    target: BuiltinTarget,
    path: String,
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
    /// One-line help message for the path prompt, including an
    /// example. Examples aren't defaults — they show shape, not the
    /// expected answer, since "the right" generated-output path is
    /// project-specific.
    fn path_help(self) -> &'static str {
        match self {
            BuiltinTarget::TypeScript => {
                "A directory; primate writes one .ts per namespace plus an index.ts. \
                 e.g. web/src/generated/constants/"
            }
            BuiltinTarget::Rust => {
                "A single .rs file with one `pub mod` block per namespace. \
                 e.g. src/generated/constants.rs"
            }
            BuiltinTarget::Python => {
                "A directory; one .py per namespace plus an __init__.py. \
                 e.g. python/generated/constants/"
            }
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

    install_render_config();
    print_logo();

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

/// Print the primate logo in ANSI Magenta + bold. Same color rule
/// as the watch TUI uses — ANSI named colors are themed by the
/// terminal so the mark reads on both light and dark backgrounds.
fn print_logo() {
    println!();
    for line in super::logo::HEADER {
        println!("  \x1b[1;35m{}\x1b[0m", line);
    }
}

// ────────────────────────────────────────────────────────────────────
// Theme
// ────────────────────────────────────────────────────────────────────

/// Set up a coherent visual palette for every prompt. Uses ANSI-named
/// colors so each terminal renders them within its own scheme; avoids
/// fixed RGB so the prompts read on both light and dark backgrounds.
fn install_render_config() {
    let cfg = RenderConfig::default()
        // The leading `?` on an unanswered prompt and the `✓` on an
        // answered one are the most visible glyphs; magenta + green
        // give the screen a clear "in progress" → "done" rhythm.
        .with_prompt_prefix(Styled::new("?").with_fg(Color::LightMagenta))
        .with_answered_prompt_prefix(Styled::new("✓").with_fg(Color::LightGreen))
        // The user's typed text — bold so it stands out from the
        // surrounding chrome.
        .with_text_input(StyleSheet::default().with_attr(inquire::ui::Attributes::BOLD))
        // Help line below each prompt.
        .with_help_message(StyleSheet::default().with_fg(Color::DarkGrey))
        // Highlighted option in selects/multiselects.
        .with_highlighted_option_prefix(Styled::new("›").with_fg(Color::LightMagenta))
        .with_selected_option(Some(
            StyleSheet::default()
                .with_fg(Color::LightMagenta)
                .with_attr(inquire::ui::Attributes::BOLD),
        ))
        // Selected items in a MultiSelect (the things with checkmarks).
        .with_selected_checkbox(Styled::new("[x]").with_fg(Color::LightMagenta))
        .with_unselected_checkbox(Styled::new("[ ]").with_fg(Color::DarkGrey));
    inquire::set_global_render_config(cfg);
}

// ────────────────────────────────────────────────────────────────────
// Prompts
// ────────────────────────────────────────────────────────────────────

fn prompt() -> Result<Answers, Box<dyn std::error::Error>> {
    let input_dir = Text::new("Where do your .prim source files live?")
        .with_help_message(
            "A directory under the project root, e.g. constants/ or shared/constants/.",
        )
        .with_autocomplete(PathAutocomplete::new())
        .with_validator(ValueRequiredValidator::default())
        .prompt()?;
    note_will_be_created(&input_dir, PathRole::Input);

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
                .with_help_message("Shows up in primate.toml — e.g. lua, kotlin, csharp.")
                .with_validator(ValueRequiredValidator::default())
                .prompt()?;
            let command = Text::new("Command")
                .with_help_message("Executable on PATH, or an absolute path.")
                .with_autocomplete(PathAutocomplete::new())
                .with_validator(ValueRequiredValidator::default())
                .prompt()?;
            let path = Text::new("Where should it write its output?")
                .with_help_message("File or directory — your plugin decides.")
                .with_autocomplete(PathAutocomplete::new())
                .with_validator(ValueRequiredValidator::default())
                .prompt()?;
            note_will_be_created(&path, PathRole::Output);
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

/// Per-target sub-flow. We don't ask about style options here —
/// every option each generator accepts lands in the generated file
/// with its default, so the user sees what's available and tunes it
/// in-place. The only thing we still need from the user is where to
/// write output.
fn configure_target(target: BuiltinTarget) -> Result<BuiltinChoice, Box<dyn std::error::Error>> {
    let path = Text::new(&format!("Where should the {} output go?", target.pretty()))
        .with_help_message(target.path_help())
        .with_autocomplete(PathAutocomplete::new())
        .with_validator(ValueRequiredValidator::default())
        .prompt()?;
    note_will_be_created(&path, PathRole::Output);

    Ok(BuiltinChoice { target, path })
}

/// Distinguish "input directory" from "output path" so the
/// post-prompt hint is accurate. The input dir has to be filled in
/// by the user; output paths get auto-created on `primate build`.
#[derive(Copy, Clone)]
enum PathRole {
    Input,
    Output,
}

/// Print a small hint right after a path prompt confirms, when the
/// path doesn't exist yet. Tells the user what's about to happen so
/// a typo (or a deliberately new path) doesn't surprise them.
fn note_will_be_created(path: &str, role: PathRole) {
    if Path::new(path).exists() {
        return;
    }
    let msg = match role {
        PathRole::Input => "Doesn't exist yet — create it before running `primate build`.",
        PathRole::Output => "Doesn't exist yet — primate will create it on `primate build`.",
    };
    // Indent so the hint hangs under the prompt's value column;
    // dark-grey so it reads as secondary.
    eprintln!("\x1b[90m  {}\x1b[0m", msg);
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
            // Pad the key column so values align across options.
            s.push_str(&format!(
                "options.{:11} = {:<25} # {}\n",
                key,
                quote_toml(default),
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

    fn ts_choice(path: &str) -> BuiltinChoice {
        BuiltinChoice {
            target: BuiltinTarget::TypeScript,
            path: path.into(),
        }
    }

    #[test]
    fn renders_every_option_at_its_default() {
        let a = Answers {
            input_dir: "constants".into(),
            builtins: vec![ts_choice("web/src/generated/constants/")],
            plugins: Vec::new(),
        };
        let out = render_toml(&a);
        // Every option each picked generator accepts shows up so the
        // user discovers what's tunable without reading the docs.
        assert!(out.contains("options.naming      = \"camelCase\""));
        assert!(out.contains("options.duration    = \"number\""));
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
