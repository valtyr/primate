//! Watch-mode TUI.
//!
//! A small ratatui app that watches the input directory, re-runs the
//! generator on changes, and displays the most recent build's status
//! source-file by source-file, with each file's diagnostics nested
//! beneath it. Intended for `primate generate --watch` only — every
//! other command stays plain text.

use crate::config::Config;
use crate::diagnostics::{Diagnostic as PrimateDiagnostic, Severity};
use crate::generators::Generator;
use crate::generators::python::PythonGenerator;
use crate::generators::rust::RustGenerator;
use crate::generators::typescript::TypeScriptGenerator;
use crate::ir::{CodeGenRequest, GeneratedFile};
use crate::parser::{ConstFile, discover_files, parse_project};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::{Frame, Terminal};
use std::collections::HashMap;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use super::logo::HEADER;

/// Snapshot of the most recent build. The TUI redraws against this
/// after every build completes.
#[derive(Default, Clone)]
struct BuildSnapshot {
    duration: Duration,
    /// Every `.prim` file discovered under `input` at build time, in
    /// no particular order. The TUI sorts and buckets these by status.
    sources: Vec<PathBuf>,
    /// Generated file paths, in the order the generators emitted them.
    generated: Vec<String>,
    /// All diagnostics from the parse/lower phases.
    diagnostics: Vec<PrimateDiagnostic>,
    /// Iff there are no error-severity diagnostics. Warnings still
    /// count as success.
    success: bool,
}

/// All state the TUI renders against.
struct App {
    config_path: PathBuf,
    input_dir: PathBuf,
    last: Option<BuildSnapshot>,
    building: bool,
    spinner_phase: usize,
    /// Set when the user requests an explicit rebuild via `r`.
    pending_rebuild: bool,
}

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn run(
    config_path: PathBuf,
    input_override: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load(&config_path)?;
    let input_dir = input_override
        .clone()
        .unwrap_or_else(|| config.input.clone());

    // Set up the file watcher before we touch the terminal so a watcher
    // failure doesn't leave the terminal in raw mode.
    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(Duration::from_millis(250), tx)?;
    debouncer
        .watcher()
        .watch(&input_dir, RecursiveMode::Recursive)?;

    let mut terminal = setup_terminal()?;
    let mut app = App {
        config_path: config_path.clone(),
        input_dir: input_dir.clone(),
        last: None,
        building: true,
        spinner_phase: 0,
        pending_rebuild: false,
    };

    // Initial build so the user sees real state immediately.
    app.last = Some(do_build(&config_path));
    app.building = false;

    let result = run_loop(&mut terminal, &mut app, &rx);

    restore_terminal(&mut terminal)?;
    drop(debouncer);
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    rx: &Receiver<notify_debouncer_mini::DebounceEventResult>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| draw(f, app))?;

        // Coalesce file events: drain the channel before triggering a
        // build, so a burst of saves becomes one rebuild.
        let mut should_build = app.pending_rebuild;
        app.pending_rebuild = false;
        match rx.try_recv() {
            Ok(_) => should_build = true,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
        while rx.try_recv().is_ok() {
            should_build = true;
        }

        if should_build {
            app.building = true;
            terminal.draw(|f| draw(f, app))?;
            app.last = Some(do_build(&app.config_path));
            app.building = false;
            continue;
        }

        // Poll keyboard for ~100ms before redrawing the spinner.
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                // Quit on q / Esc / Ctrl-C / Ctrl-D. Raw mode swallows the
                // SIGINT/SIGQUIT a non-raw terminal would have produced,
                // so we have to recognize the key combos explicitly.
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c') | KeyCode::Char('d') if ctrl => return Ok(()),
                    KeyCode::Char('r') => app.pending_rebuild = true,
                    _ => {}
                }
            }
        } else {
            app.spinner_phase = (app.spinner_phase + 1) % SPINNER.len();
        }
    }
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>, Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────
// Rendering
// ────────────────────────────────────────────────────────────────────

/// Width of the braille banner in monospace cells. Each braille glyph
/// (U+28xx) is single-width in every modern terminal font.
const LOGO_COLS: u16 = 34;

/// Minimum body width we'll allow alongside the logo — narrower than
/// this and the source list starts wrapping awkwardly. Sum with the
/// logo width plus a 2-column gutter to get the threshold below which
/// the logo is hidden entirely.
const MIN_BODY_COLS: u16 = 44;

/// Show the logo iff there's room for it next to a usable body
/// column.
fn should_show_logo(width: u16) -> bool {
    width >= LOGO_COLS + MIN_BODY_COLS + 2
}

fn draw(f: &mut Frame, app: &App) {
    let size = f.area();
    let show_logo = should_show_logo(size.width);
    // The top zone is the height of the logo when the logo fits, or
    // a single row (status only) when it doesn't.
    let top_height = if show_logo { HEADER.len() as u16 } else { 1 };

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_height),
            Constraint::Min(0),    // body continuation
            Constraint::Length(1), // footer
        ])
        .split(size);
    let top_zone = v[0];
    let body_continuation = v[1];
    let footer_zone = v[2];

    // Status is a single line that always sits at the very top-left.
    // The body content (sources + diagnostics + outputs) starts on the
    // line below the status; whatever doesn't fit beside the logo
    // overflows into the body-continuation zone.
    let status_line = build_status_line(app);
    let body_lines = build_body_lines(app);

    if show_logo {
        // Horizontal split: body lives on the left, the logo floats on
        // the right with a one-cell gutter.
        let h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(1), // gutter
                Constraint::Length(LOGO_COLS),
            ])
            .split(top_zone);
        let left = h[0];
        let logo_area = h[2];

        // Inside the left column: row 0 is status, rows 1.. are the
        // first chunk of body lines.
        let lv = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(left);
        let status_area = lv[0];
        let top_body_area = lv[1];

        f.render_widget(Paragraph::new(status_line), status_area);

        let top_body_height = top_body_area.height as usize;
        let (top_chunk, rest_chunk): (Vec<Line>, Vec<Line>) = if body_lines.len() > top_body_height
        {
            (
                body_lines[..top_body_height].to_vec(),
                body_lines[top_body_height..].to_vec(),
            )
        } else {
            (body_lines.clone(), Vec::new())
        };
        f.render_widget(Paragraph::new(top_chunk), top_body_area);
        f.render_widget(Paragraph::new(rest_chunk), body_continuation);

        f.render_widget(logo_paragraph(), logo_area);
    } else {
        // Narrow terminal: skip the logo entirely. Status on top, body
        // fills the rest.
        f.render_widget(Paragraph::new(status_line), top_zone);
        f.render_widget(Paragraph::new(body_lines), body_continuation);
    }

    draw_footer(f, footer_zone);
}

fn logo_paragraph() -> Paragraph<'static> {
    let style = Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::BOLD);
    let lines: Vec<Line> = HEADER
        .iter()
        .map(|line| Line::from(Span::styled(*line, style)))
        .collect();
    Paragraph::new(lines)
}

fn build_status_line(app: &App) -> Line<'static> {
    let dim = Style::default().fg(Color::DarkGray);
    let mut spans = vec![
        Span::styled("watching ", dim),
        Span::styled(
            app.input_dir.display().to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
    ];

    if app.building {
        spans.push(Span::styled(
            format!("{} rebuilding…", SPINNER[app.spinner_phase]),
            Style::default().fg(Color::Yellow),
        ));
    } else if let Some(last) = &app.last {
        let (glyph, color) = if last.success {
            ("✓", Color::Green)
        } else {
            ("✗", Color::Red)
        };
        spans.push(Span::styled(
            format!("{} {}", glyph, fmt_duration(last.duration)),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));

        let (errors, warnings) = count_severities(&last.diagnostics);
        if errors > 0 {
            spans.push(Span::styled(
                format!(
                    " · {} {}",
                    errors,
                    if errors == 1 { "error" } else { "errors" }
                ),
                Style::default().fg(Color::Red),
            ));
        }
        if warnings > 0 {
            spans.push(Span::styled(
                format!(
                    " · {} {}",
                    warnings,
                    if warnings == 1 { "warning" } else { "warnings" }
                ),
                Style::default().fg(Color::Yellow),
            ));
        }
        spans.push(Span::styled(
            format!(
                " · {} src · {} out",
                last.sources.len(),
                last.generated.len()
            ),
            dim,
        ));
    } else {
        spans.push(Span::styled("—", dim));
    }
    Line::from(spans)
}

fn build_body_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();
    let dim = Style::default().fg(Color::DarkGray);

    let Some(last) = &app.last else {
        lines.push(Line::from(Span::styled("(waiting for first build)", dim)));
        return lines;
    };

    // SOURCES ─ bucketed by severity (errors first), each .prim file
    // gets a dot prefix and any of its diagnostics nested below.
    let buckets = bucket_sources(&last.sources, &last.diagnostics, &app.input_dir);
    for entry in &buckets {
        let dot_style = match entry.status {
            Status::Error => Style::default().fg(Color::Red),
            Status::Warning => Style::default().fg(Color::Yellow),
            Status::Clean => Style::default().fg(Color::Green),
        };
        lines.push(Line::from(vec![
            Span::styled("● ", dot_style),
            Span::raw(entry.display_path.clone()),
        ]));
        // Diagnostics are sorted error-then-warning, then by line.
        let mut diags = entry.diagnostics.clone();
        diags.sort_by_key(|d| {
            let sev_rank: u8 = match d.severity {
                Severity::Error => 0,
                Severity::Warning => 1,
                Severity::Info => 2,
            };
            (sev_rank, d.line, d.column)
        });
        for d in diags {
            lines.push(diag_line(d));
        }
    }

    // OUTPUTS ─ compact list of generated file paths under a single
    // dim heading. Skipped on a failed build (no outputs were written).
    if !last.generated.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("outputs ({})", last.generated.len()),
            dim,
        )));
        for path in &last.generated {
            lines.push(Line::from(vec![
                Span::styled("→ ", dim),
                Span::raw(short_output_path(path)),
            ]));
        }
    }

    lines
}

fn draw_footer(f: &mut Frame, area: Rect) {
    let dim = Style::default().fg(Color::DarkGray);
    let key = Style::default()
        .fg(Color::Black)
        .bg(Color::Gray)
        .add_modifier(Modifier::BOLD);
    let footer = Line::from(vec![
        Span::styled(" q ", key),
        Span::styled(" quit", dim),
        Span::styled("   ", dim),
        Span::styled(" r ", key),
        Span::styled(" rebuild", dim),
    ]);
    f.render_widget(Paragraph::new(footer), area);
}

fn diag_line(d: &PrimateDiagnostic) -> Line<'static> {
    let (glyph, color) = match d.severity {
        Severity::Error => ("✘", Color::Red),
        Severity::Warning => ("⚠", Color::Yellow),
        Severity::Info => ("ℹ", Color::Cyan),
    };
    let dim = Style::default().fg(Color::DarkGray);
    Line::from(vec![
        Span::raw("    "),
        Span::styled(glyph.to_string(), Style::default().fg(color)),
        Span::raw(" "),
        Span::styled(format!("L{}:{}", d.line, d.column), dim),
        Span::raw("  "),
        Span::styled(
            d.code.clone(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(d.message.clone()),
    ])
}

// ────────────────────────────────────────────────────────────────────
// Bucketing & path helpers
// ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Error,
    Warning,
    Clean,
}

#[derive(Clone)]
struct SourceEntry<'a> {
    /// The displayed name for the file — relative to the input dir
    /// when possible, falling back to the file name only.
    display_path: String,
    status: Status,
    diagnostics: Vec<&'a PrimateDiagnostic>,
}

/// Group every diagnostic with its owning `.prim` file, classify each
/// file by worst severity, and sort: errors first, warnings second,
/// clean last; alphabetical by display path within each bucket.
fn bucket_sources<'a>(
    sources: &[PathBuf],
    diagnostics: &'a [PrimateDiagnostic],
    input_dir: &Path,
) -> Vec<SourceEntry<'a>> {
    // Match diagnostics to source files via canonical paths; fall
    // back to the literal `file` field on the diagnostic if a match
    // fails (e.g. a config-error pointing at primate.toml).
    let mut by_path: HashMap<PathBuf, Vec<&PrimateDiagnostic>> = HashMap::new();
    for d in diagnostics {
        let p = PathBuf::from(&d.file);
        let canon = std::fs::canonicalize(&p).unwrap_or(p);
        by_path.entry(canon).or_default().push(d);
    }

    // Build the entry for every known source. Sources without
    // diagnostics still render (as green dots).
    let mut entries: Vec<SourceEntry<'a>> = Vec::with_capacity(sources.len());
    for source in sources {
        let canon = std::fs::canonicalize(source).unwrap_or_else(|_| source.clone());
        let diags = by_path.remove(&canon).unwrap_or_default();
        let status = classify(&diags);
        entries.push(SourceEntry {
            display_path: short_source_path(source, input_dir),
            status,
            diagnostics: diags,
        });
    }

    // Any leftover diagnostics (whose `file` didn't match a known
    // source) become their own entries — typically primate.toml or
    // similar.
    for (path, diags) in by_path.into_iter() {
        let status = classify(&diags);
        entries.push(SourceEntry {
            display_path: short_source_path(&path, input_dir),
            status,
            diagnostics: diags,
        });
    }

    entries.sort_by(|a, b| {
        let rank = |s: Status| match s {
            Status::Error => 0,
            Status::Warning => 1,
            Status::Clean => 2,
        };
        rank(a.status)
            .cmp(&rank(b.status))
            .then(a.display_path.cmp(&b.display_path))
    });
    entries
}

fn classify(diags: &[&PrimateDiagnostic]) -> Status {
    if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        Status::Error
    } else if diags
        .iter()
        .any(|d| matches!(d.severity, Severity::Warning))
    {
        Status::Warning
    } else {
        Status::Clean
    }
}

fn count_severities(diags: &[PrimateDiagnostic]) -> (usize, usize) {
    let mut errors = 0;
    let mut warnings = 0;
    for d in diags {
        match d.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
            Severity::Info => {}
        }
    }
    (errors, warnings)
}

/// Strip the input-directory prefix so `examples/constants/limits.prim`
/// renders as just `limits.prim`. Falls back to the file name if the
/// path isn't under `input_dir`.
fn short_source_path(path: &Path, input_dir: &Path) -> String {
    if let (Ok(canon), Ok(input_canon)) = (
        std::fs::canonicalize(path),
        std::fs::canonicalize(input_dir),
    ) && let Ok(rel) = canon.strip_prefix(&input_canon)
    {
        return rel.display().to_string();
    }
    path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Strip a leading `./` from generated paths when present; everything
/// else stays as-is. Output paths are usually short enough to fit.
fn short_output_path(path: &str) -> String {
    path.strip_prefix("./").unwrap_or(path).to_string()
}

fn fmt_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{} ms", ms)
    } else {
        format!("{:.2}s", d.as_secs_f64())
    }
}

// ────────────────────────────────────────────────────────────────────
// Build pipeline (mirrors run_generate but captures outputs in memory)
// ────────────────────────────────────────────────────────────────────

fn do_build(config_path: &Path) -> BuildSnapshot {
    let started = Instant::now();
    let mut snap = BuildSnapshot::default();

    let config = match Config::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            snap.diagnostics.push(synthetic_error(
                config_path,
                format!("loading config: {}", e),
            ));
            snap.duration = started.elapsed();
            return snap;
        }
    };

    let files = match discover_files(&config.input) {
        Ok(f) => f,
        Err(e) => {
            snap.diagnostics.push(synthetic_error(
                &config.input,
                format!("scanning input: {}", e),
            ));
            snap.duration = started.elapsed();
            return snap;
        }
    };

    snap.sources = files.iter().map(|f: &ConstFile| f.path.clone()).collect();

    let project = parse_project(files);
    snap.diagnostics
        .extend(project.diagnostics.diagnostics.iter().cloned());

    if project.diagnostics.has_errors() {
        snap.duration = started.elapsed();
        return snap;
    }

    for output_config in &config.outputs {
        let output_path = output_config.path.display().to_string();
        let options: HashMap<String, serde_json::Value> = output_config
            .options
            .iter()
            .map(|(k, v)| (k.clone(), super::toml_to_json(v)))
            .collect();

        let mut request = CodeGenRequest::new(output_path.clone(), options.clone());
        request.modules = project.modules.clone();
        request.enums = project.enums.clone();
        request.aliases = project.aliases.clone();

        let response_files: Vec<GeneratedFile> = match output_config.generator.as_deref() {
            Some("typescript") => {
                TypeScriptGenerator::from_options(&options)
                    .generate(&request)
                    .files
            }
            Some("rust") => {
                RustGenerator::from_options(&options)
                    .generate(&request)
                    .files
            }
            Some("python") => {
                PythonGenerator::from_options(&options)
                    .generate(&request)
                    .files
            }
            Some(other) => {
                snap.diagnostics.push(synthetic_error(
                    config_path,
                    format!("unknown generator `{}`", other),
                ));
                continue;
            }
            None => continue,
        };

        for file in response_files {
            if let Some(parent) = Path::new(&file.path).parent()
                && !parent.as_os_str().is_empty()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                snap.diagnostics
                    .push(synthetic_error(parent, format!("creating dir: {}", e)));
                continue;
            }
            if let Err(e) = std::fs::write(&file.path, &file.content) {
                snap.diagnostics.push(synthetic_error(
                    Path::new(&file.path),
                    format!("writing file: {}", e),
                ));
                continue;
            }
            snap.generated.push(file.path);
        }
    }

    snap.success = !snap
        .diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));
    snap.duration = started.elapsed();
    snap
}

fn synthetic_error(path: &Path, message: String) -> PrimateDiagnostic {
    PrimateDiagnostic {
        file: path.display().to_string(),
        line: 1,
        column: 1,
        length: None,
        severity: Severity::Error,
        code: "io-error".to_string(),
        message,
        targets: vec![],
    }
}
