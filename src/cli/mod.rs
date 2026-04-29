//! CLI interface for primate
//!
//! Implements command-line argument parsing and command dispatch.

use crate::config::Config;
use crate::diagnostics::{Diagnostics, Severity};
use crate::generators::python::PythonGenerator;
use crate::generators::rust::RustGenerator;
use crate::generators::typescript::TypeScriptGenerator;
use crate::generators::Generator;
use crate::ir::CodeGenRequest;
use crate::parser::{discover_files, parse_project, ParsedProject};
use crate::sourcemap::{Sourcemap, SourcemapEntry};
use clap::{Parser, Subcommand};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::Duration;

/// Cross-language constant transpiler
#[derive(Parser)]
#[command(name = "primate")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "primate.toml")]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Validate constants without generating output
    // r[impl cli.check]
    Check {
        /// Watch for changes and re-validate
        // r[impl cli.check-watch]
        #[arg(long)]
        watch: bool,

        /// Output format
        // r[impl cli.check-format]
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Start the LSP server
    // r[impl cli.lsp]
    Lsp {
        /// Use stdio transport (default)
        #[arg(long)]
        stdio: bool,
    },

    /// Generate constants (default command)
    Generate {
        /// Input directory (overrides config)
        #[arg(short, long)]
        input: Option<PathBuf>,

        /// Output specification (e.g., ts:./out.ts)
        #[arg(short, long)]
        output: Option<String>,

        /// Watch for changes and regenerate
        // r[impl cli.generate-watch]
        #[arg(long)]
        watch: bool,
    },

    /// Format `.prim` files in place. With `--check`, exit non-zero if any
    /// file is not already formatted.
    Fmt {
        /// Files or directories to format. Defaults to the input directory
        /// from the active config.
        paths: Vec<PathBuf>,

        /// Don't write changes; exit non-zero if any file would be reformatted.
        #[arg(long)]
        check: bool,
    },
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Check { watch, format }) => {
            if watch {
                run_check_watch(&cli.config, &format)?;
            } else {
                run_check(&cli.config, &format)?;
            }
        }
        Some(Command::Lsp { stdio: _ }) => {
            crate::lsp::run_server(&cli.config)?;
        }
        Some(Command::Generate {
            input,
            output,
            watch,
        }) => {
            if watch {
                run_generate_watch(&cli.config, input)?;
            } else {
                run_generate(&cli.config, input, output)?;
            }
        }
        Some(Command::Fmt { paths, check }) => {
            run_fmt(&cli.config, paths, check)?;
        }
        None => {
            // r[impl cli.default-config]
            // Default: run generate with config
            run_generate(&cli.config, None, None)?;
        }
    }

    Ok(())
}

fn run_check(config_path: &PathBuf, format: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load(config_path)?;
    let files = discover_files(&config.input)?;

    if files.is_empty() {
        eprintln!("No .prim files found in {}", config.input.display());
        return Ok(());
    }

    let project = parse_project(files);

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&project.diagnostics)?);
    } else {
        print_diagnostics(&project.diagnostics);
    }

    if project.diagnostics.has_errors() {
        std::process::exit(1);
    }

    eprintln!(
        "Checked {} modules, {} constants, {} enums",
        project.modules.len(),
        project
            .modules
            .iter()
            .map(|m| m.constants.len())
            .sum::<usize>(),
        project.enums.len()
    );

    Ok(())
}

// r[impl cli.check-watch]
fn run_check_watch(config_path: &PathBuf, format: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load(config_path)?;

    // Initial check
    eprintln!("Watching {} for changes...", config.input.display());
    let _ = run_check(config_path, format);

    // Set up file watcher
    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)?;

    debouncer
        .watcher()
        .watch(&config.input, RecursiveMode::Recursive)?;

    loop {
        match rx.recv() {
            Ok(Ok(_events)) => {
                eprintln!("\n--- File changed, re-checking ---\n");
                let _ = run_check(config_path, format);
            }
            Ok(Err(e)) => eprintln!("Watch error: {:?}", e),
            Err(e) => {
                eprintln!("Channel error: {:?}", e);
                break;
            }
        }
    }

    Ok(())
}

fn run_generate(
    config_path: &PathBuf,
    input_override: Option<PathBuf>,
    _output_override: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load(config_path)?;
    let input_dir = input_override.as_ref().unwrap_or(&config.input);

    let files = discover_files(input_dir)?;

    if files.is_empty() {
        eprintln!("No .prim files found in {}", input_dir.display());
        return Ok(());
    }

    let project = parse_project(files);

    if project.diagnostics.has_errors() {
        print_diagnostics(&project.diagnostics);
        std::process::exit(1);
    }

    print_diagnostics(&project.diagnostics);

    // r[impl sourcemap.json]
    let mut sourcemap = Sourcemap::new();

    for output_config in &config.outputs {
        let output_path = output_config.path.display().to_string();

        let options: HashMap<String, serde_json::Value> = output_config
            .options
            .iter()
            .map(|(k, v)| (k.clone(), toml_to_json(v)))
            .collect();

        let mut request = CodeGenRequest::new(output_path.clone(), options.clone());
        request.modules = project.modules.clone();
        request.enums = project.enums.clone();
        request.aliases = project.aliases.clone();

        if let Some(ref generator_name) = output_config.generator {
            // Validate that `path` matches what the generator expects. The
            // generators emit one (rust) or many (ts, python) files into
            // `path`; getting the shape wrong produces opaque OS errors
            // ("Is a directory", "Not a directory") down the line, so catch
            // it here with a message that points at the config field.
            if let Err(msg) = validate_output_path(generator_name, &output_path) {
                eprintln!("Config error in [[output]] generator = \"{}\": {}", generator_name, msg);
                std::process::exit(2);
            }

            match generator_name.as_str() {
                "typescript" => {
                    let generator = TypeScriptGenerator::from_options(&options);
                    let response = generator.generate(&request);
                    write_response_files(generator_name, &response.files, &mut sourcemap, &project)?;
                }
                "rust" => {
                    let generator = RustGenerator::from_options(&options);
                    let response = generator.generate(&request);
                    write_response_files(generator_name, &response.files, &mut sourcemap, &project)?;
                }
                "python" => {
                    let generator = PythonGenerator::from_options(&options);
                    let response = generator.generate(&request);
                    write_response_files(generator_name, &response.files, &mut sourcemap, &project)?;
                }
                _ => {
                    eprintln!("Unknown generator: {}", generator_name);
                }
            }
        } else if let Some(ref plugin_name) = output_config.plugin {
            match crate::plugin::resolve_plugin(plugin_name) {
                Ok(plugin_path) => match crate::plugin::invoke_plugin(&plugin_path, &request) {
                    Ok(response) => {
                        write_response_files(plugin_name, &response.files, &mut sourcemap, &project)?;
                        for error in response.errors {
                            eprintln!("Plugin error: {}", error.message);
                        }
                    }
                    Err(e) => eprintln!("Plugin execution failed: {}", e),
                },
                Err(e) => eprintln!("Plugin resolution failed: {}", e),
            }
        }
    }

    // Write sourcemap
    // r[impl sourcemap.json]
    if !sourcemap.entries.is_empty() {
        let sourcemap_path = config.sourcemap_path(config_path);
        std::fs::write(&sourcemap_path, sourcemap.to_json()?)
            .map_err(|e| format!("writing sourcemap to {}: {}", sourcemap_path.display(), e))?;
        eprintln!("Generated: {}", sourcemap_path.display());
    }

    Ok(())
}

/// Check that the configured `path` matches what the generator expects.
/// `rust` writes one file; `typescript` and `python` write directories.
fn validate_output_path(generator: &str, path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    let looks_like_dir = path.ends_with('/')
        || path.ends_with(std::path::MAIN_SEPARATOR)
        || (p.extension().is_none() && !path.is_empty());
    let exists_as_dir = p.is_dir();
    let exists_as_file = p.is_file();

    match generator {
        "rust" => {
            if exists_as_dir {
                return Err(format!(
                    "path = {:?} is a directory but the rust generator emits a single .rs file. \
                     Set path to something like \"src/generated/constants.rs\".",
                    path,
                ));
            }
            if looks_like_dir && !exists_as_file {
                return Err(format!(
                    "path = {:?} looks like a directory; the rust generator emits a single .rs file. \
                     Set path to something like \"src/generated/constants.rs\".",
                    path,
                ));
            }
            Ok(())
        }
        "typescript" | "python" => {
            if exists_as_file {
                return Err(format!(
                    "path = {:?} is an existing file but the {} generator emits a directory of files. \
                     Set path to a directory (e.g. \"web/src/generated/constants/\").",
                    path, generator,
                ));
            }
            // Heuristic: a non-existent path with an extension and no trailing
            // slash probably came from the old single-file mode — surface it
            // before producing a confusing nested file like `foo.ts/limits.ts`.
            if !exists_as_dir && p.extension().is_some() && !looks_like_dir {
                return Err(format!(
                    "path = {:?} looks like a file but the {} generator emits a directory of files. \
                     Set path to a directory (e.g. \"web/src/generated/constants/\").",
                    path, generator,
                ));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Write each generated file to disk, attaching the path and generator name
/// to any I/O error so the user sees what failed.
fn write_response_files(
    generator: &str,
    files: &[crate::ir::GeneratedFile],
    sourcemap: &mut Sourcemap,
    project: &ParsedProject,
) -> Result<(), Box<dyn std::error::Error>> {
    for file in files {
        if let Some(parent) = std::path::Path::new(&file.path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    format!(
                        "creating output directory {} for {} generator: {}",
                        parent.display(),
                        generator,
                        e,
                    )
                })?;
            }
        }
        std::fs::write(&file.path, &file.content).map_err(|e| {
            format!(
                "writing {} (from {} generator): {}",
                file.path, generator, e,
            )
        })?;
        eprintln!("Generated: {}", file.path);
        add_sourcemap_entries(sourcemap, project, file);
    }
    Ok(())
}

// r[impl cli.generate-watch]
fn run_generate_watch(
    config_path: &PathBuf,
    input_override: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load(config_path)?;
    let input_dir = input_override.as_ref().unwrap_or(&config.input).clone();

    // Initial generate
    eprintln!("Watching {} for changes...", input_dir.display());
    let _ = run_generate(config_path, input_override.clone(), None);

    // Set up file watcher
    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)?;

    debouncer
        .watcher()
        .watch(&input_dir, RecursiveMode::Recursive)?;

    loop {
        match rx.recv() {
            Ok(Ok(_events)) => {
                eprintln!("\n--- File changed, regenerating ---\n");
                let _ = run_generate(config_path, input_override.clone(), None);
            }
            Ok(Err(e)) => eprintln!("Watch error: {:?}", e),
            Err(e) => {
                eprintln!("Channel error: {:?}", e);
                break;
            }
        }
    }

    Ok(())
}

fn run_fmt(
    config_path: &PathBuf,
    paths: Vec<PathBuf>,
    check: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Resolve target paths: explicit args win, otherwise fall back to the input dir.
    let targets: Vec<PathBuf> = if paths.is_empty() {
        let config = Config::load(config_path).ok();
        match config {
            Some(c) => vec![c.input],
            None => {
                eprintln!(
                    "no paths provided and no config found at {}",
                    config_path.display()
                );
                std::process::exit(2);
            }
        }
    } else {
        paths
    };

    // Expand directories to .prim files.
    let mut files: Vec<PathBuf> = Vec::new();
    for t in &targets {
        if t.is_file() {
            files.push(t.clone());
        } else if t.is_dir() {
            for entry in walkdir::WalkDir::new(t).follow_links(true).into_iter().filter_map(|e| e.ok()) {
                let p = entry.path();
                if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("const") {
                    files.push(p.to_path_buf());
                }
            }
        } else {
            eprintln!("path not found: {}", t.display());
            std::process::exit(2);
        }
    }

    if files.is_empty() {
        eprintln!("no .prim files to format");
        return Ok(());
    }

    let mut had_diff = false;
    let mut had_error = false;
    for path in &files {
        let original = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: {}", path.display(), e);
                had_error = true;
                continue;
            }
        };
        match crate::formatter::format_source(&original) {
            Ok(formatted) => {
                if formatted != original {
                    had_diff = true;
                    if check {
                        eprintln!("would reformat: {}", path.display());
                    } else {
                        std::fs::write(path, &formatted)?;
                        eprintln!("formatted: {}", path.display());
                    }
                }
            }
            Err(diags) => {
                for diag in diags.diagnostics {
                    eprintln!(
                        "{}:{}:{}: error: {}",
                        path.display(),
                        diag.line,
                        diag.column,
                        diag.message
                    );
                }
                had_error = true;
            }
        }
    }

    if had_error {
        std::process::exit(1);
    }
    if check && had_diff {
        std::process::exit(1);
    }
    Ok(())
}

fn add_sourcemap_entries(
    sourcemap: &mut Sourcemap,
    project: &ParsedProject,
    generated_file: &crate::ir::GeneratedFile,
) {
    let mut symbol_to_source = HashMap::new();
    for module in &project.modules {
        for constant in &module.constants {
            symbol_to_source.insert(
                format!("{}.{}", module.namespace, constant.name),
                (
                    &constant.source.file,
                    constant.source.line,
                    constant.source.column,
                ),
            );
        }
    }
    for enum_def in &project.enums {
        symbol_to_source.insert(
            format!("{}.{}", enum_def.namespace, enum_def.name),
            (
                &enum_def.source.file,
                enum_def.source.line,
                enum_def.source.column,
            ),
        );
    }

    for mapping in &generated_file.mappings {
        if let Some((source_file, source_line, source_column)) =
            symbol_to_source.get(&mapping.symbol)
        {
            sourcemap.add_entry(SourcemapEntry {
                symbol: mapping.symbol.clone(),
                source_file: (*source_file).clone(),
                source_line: *source_line,
                source_column: *source_column,
                output_file: generated_file.path.clone(),
                output_line: mapping.line,
                output_column: mapping.column,
            });
        }
    }
}

fn print_diagnostics(diagnostics: &Diagnostics) {
    for diag in &diagnostics.diagnostics {
        let level = match diag.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };

        eprintln!(
            "{}:{}:{}: {}: [{}] {}",
            diag.file, diag.line, diag.column, level, diag.code, diag.message
        );
    }
}

fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Array(arr) => serde_json::Value::Array(arr.iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            let map: serde_json::Map<String, serde_json::Value> = t
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}
