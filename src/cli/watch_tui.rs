//! Watch-mode TUI.
//!
//! A small ratatui app that watches the input directory, re-runs the
//! generator on changes, and displays the most recent build's status,
//! generated files, and diagnostics. Intended for `primate generate
//! --watch` only — every other command stays plain text.

use crate::config::Config;
use crate::diagnostics::{Diagnostic as PrimateDiagnostic, Severity};
use crate::generators::python::PythonGenerator;
use crate::generators::rust::RustGenerator;
use crate::generators::typescript::TypeScriptGenerator;
use crate::generators::Generator;
use crate::ir::{CodeGenRequest, GeneratedFile};
use crate::parser::{discover_files, parse_project};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::collections::HashMap;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

/// The ASCII banner — six rows of braille glyphs depicting the primate
/// mark. Width is fixed at 34 columns; the TUI hides the whole header
/// pane when the terminal is narrower than that.
const HEADER: &[&str] = &[
    "⠀⠀⠀⠀⣠⣶⣿⣿⣷⣦⣀⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
    "⠀⠀⣴⣶⣿⠋⣉⠉⣁⠙⣿⣿⡇⠀⠀⠀⠀⠀⢀⣀⣀⣀⣀⡀⠀⠀⠀⢀⣀⣀⣀⠀⠀⠀",
    "⠀⠀⠻⢿⣿⣄⠉⢤⠉⢠⣿⣏⣁⣤⣴⣶⣾⣿⣿⣿⣿⠿⠛⠻⢷⣆⠀⠘⠛⠛⠿⣿⣦⠀",
    "⠀⠀⢀⣄⠻⣿⣄⠀⢀⣼⣿⣿⣿⣿⣿⠿⠟⠋⠉⣀⣴⣾⣿⣿⣦⠉⠀⠀⠀⠀⠀⠈⣿⣧",
    "⠀⢀⣾⣿⡇⠈⢿⣿⣿⣿⣿⣿⣿⣿⣿⣶⣤⣤⣾⣿⣿⣿⠿⣿⣿⣧⠀⠀⠀⠀⠀⠀⣸⣿",
    "⢀⣾⣿⣿⣠⣴⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣟⣁⣀⢻⣿⣿⡆⠀⠀⠀⠀⢠⣿⡏",
    "⣼⣿⣿⣿⣿⣿⡿⠟⠻⢿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡆⢻⣿⣿⣌⠻⢿⣷⣶⣶⣄",
    "⠙⠿⠿⠛⠉⠁⠀⠀⠀⠀⠉⠙⠛⠛⠿⠿⠿⠿⠿⠿⠿⠿⠿⠿⠈⠻⠿⠿⠿⠈⠿⠿⠿⠋",
];

/// Snapshot of the most recent build. The TUI redraws against this
/// after every build completes.
#[derive(Default, Clone)]
struct BuildSnapshot {
    /// Wall-clock duration of the build (parse + lower + generate + write).
    duration: Duration,
    /// Generated file paths, in the order the generators emitted them.
    generated: Vec<String>,
    /// All diagnostics from the parse/lower phases. Errors gate the
    /// `success` flag; warnings flow through unchanged.
    diagnostics: Vec<PrimateDiagnostic>,
    /// True iff there were no error-severity diagnostics. A build with
    /// warnings is still successful.
    success: bool,
    /// When the build finished. Used for the "x ago" footer text.
    finished_at: Option<Instant>,
}

/// All state the TUI renders against.
struct App {
    config_path: PathBuf,
    input_dir: PathBuf,
    /// `None` until the first build completes.
    last: Option<BuildSnapshot>,
    /// True while a build is running. Drives the "rebuilding…" indicator.
    building: bool,
    /// Used to animate the spinner without holding a separate timer.
    spinner_phase: usize,
    /// Set when the user requests an explicit rebuild via `r`. The main
    /// loop catches this and runs a build before going back to the file
    /// watcher.
    pending_rebuild: bool,
}

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn run(
    config_path: PathBuf,
    input_override: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load(&config_path)?;
    let input_dir = input_override.clone().unwrap_or_else(|| config.input.clone());

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

        // Coalesce file events that arrive while we're blocked: if the
        // watcher channel produces anything, drain it before triggering a
        // build, so a burst of saves becomes a single rebuild.
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
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
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

fn draw(f: &mut Frame, app: &App) {
    let size = f.area();

    // The header is a fixed 8 rows tall. Width-wise we can render it on
    // any terminal — the glyphs degrade fine at narrow widths, and we
    // pad inside the block. We allocate `header_height + 2` for the
    // bordered block.
    let header_height = HEADER.len() as u16 + 2;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Length(3), // status line
            Constraint::Min(5),    // generated files
            Constraint::Min(3),    // diagnostics
            Constraint::Length(1), // footer
        ])
        .split(size);

    draw_header(f, chunks[0]);
    draw_status(f, chunks[1], app);
    draw_generated(f, chunks[2], app);
    draw_diagnostics(f, chunks[3], app);
    draw_footer(f, chunks[4], app);
}

fn draw_header(f: &mut Frame, area: ratatui::layout::Rect) {
    // Use ANSI `Magenta` + bold rather than the literal brand purple
    // (#3027D4) from the logo: each terminal themes the named ANSI
    // colors to fit its own foreground/background, so the header reads
    // legibly on both light and dark color schemes. The fixed RGB had
    // poor contrast (~3.5:1) on dark terminals.
    let style = Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::BOLD);
    let lines: Vec<Line> = HEADER
        .iter()
        .map(|line| Line::from(Span::styled(*line, style)))
        .collect();
    let block = Block::default().borders(Borders::BOTTOM);
    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, area);
}

fn draw_status(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled("watching ", Style::default().fg(Color::DarkGray)));
    spans.push(Span::styled(
        app.input_dir.display().to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw("   "));

    if app.building {
        let glyph = SPINNER[app.spinner_phase];
        spans.push(Span::styled(
            format!("{} rebuilding…", glyph),
            Style::default().fg(Color::Yellow),
        ));
    } else if let Some(last) = &app.last {
        let (glyph, color) = if last.success {
            ("✓", Color::Green)
        } else {
            ("✗", Color::Red)
        };
        let summary = if last.success {
            format!(
                "{} {} files in {}",
                glyph,
                last.generated.len(),
                fmt_duration(last.duration),
            )
        } else {
            let n = last
                .diagnostics
                .iter()
                .filter(|d| matches!(d.severity, Severity::Error))
                .count();
            format!(
                "{} build failed ({} {})",
                glyph,
                n,
                if n == 1 { "error" } else { "errors" }
            )
        };
        spans.push(Span::styled(summary, Style::default().fg(color)));
    } else {
        spans.push(Span::styled("—", Style::default().fg(Color::DarkGray)));
    }

    let para = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::BOTTOM))
        .wrap(Wrap { trim: true });
    f.render_widget(para, area);
}

fn draw_generated(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let items: Vec<ListItem> = match &app.last {
        Some(last) if !last.generated.is_empty() => last
            .generated
            .iter()
            .map(|p| ListItem::new(Line::from(vec![
                Span::styled("→ ", Style::default().fg(Color::DarkGray)),
                Span::raw(p.clone()),
            ])))
            .collect(),
        Some(_) => vec![ListItem::new(Span::styled(
            "(no files generated)",
            Style::default().fg(Color::DarkGray),
        ))],
        None => vec![ListItem::new(Span::styled(
            "(waiting for first build)",
            Style::default().fg(Color::DarkGray),
        ))],
    };
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" generated "),
    );
    f.render_widget(list, area);
}

fn draw_diagnostics(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let items: Vec<ListItem> = match &app.last {
        Some(last) if !last.diagnostics.is_empty() => last
            .diagnostics
            .iter()
            .map(|d| {
                let (sev, color) = match d.severity {
                    Severity::Error => ("error", Color::Red),
                    Severity::Warning => ("warn ", Color::Yellow),
                    Severity::Info => ("info ", Color::Cyan),
                };
                ListItem::new(Line::from(vec![
                    Span::styled(sev, Style::default().fg(color)),
                    Span::raw("  "),
                    Span::styled(
                        format!("{}:{}", d.file, d.line),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw("  "),
                    Span::raw(d.message.clone()),
                ]))
            })
            .collect(),
        _ => vec![ListItem::new(Span::styled(
            "(no diagnostics)",
            Style::default().fg(Color::DarkGray),
        ))],
    };
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" diagnostics "),
    );
    f.render_widget(list, area);
}

fn draw_footer(f: &mut Frame, area: ratatui::layout::Rect, _app: &App) {
    let footer = Line::from(vec![
        Span::styled(" q ", Style::default().fg(Color::Black).bg(Color::Gray)),
        Span::raw(" quit   "),
        Span::styled(" r ", Style::default().fg(Color::Black).bg(Color::Gray)),
        Span::raw(" rebuild"),
    ]);
    f.render_widget(Paragraph::new(footer), area);
}

fn fmt_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{} ms", ms)
    } else {
        format!("{:.2}s", d.as_secs_f64())
    }
}

/// Run a complete build (parse → lower → all generators → fs::write) and
/// return a snapshot describing the result. Mirrors `run_generate` in
/// the non-TUI CLI but captures the outputs in memory so the TUI can
/// render them.
fn do_build(config_path: &Path) -> BuildSnapshot {
    let started = Instant::now();
    let mut snap = BuildSnapshot::default();

    let config = match Config::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            snap.diagnostics.push(PrimateDiagnostic {
                file: config_path.display().to_string(),
                line: 1,
                column: 1,
                length: None,
                severity: Severity::Error,
                code: "config-error".to_string(),
                message: format!("loading config: {}", e),
                targets: vec![],
            });
            snap.success = false;
            snap.duration = started.elapsed();
            snap.finished_at = Some(Instant::now());
            return snap;
        }
    };

    let files = match discover_files(&config.input) {
        Ok(f) => f,
        Err(e) => {
            snap.diagnostics.push(PrimateDiagnostic {
                file: config.input.display().to_string(),
                line: 1,
                column: 1,
                length: None,
                severity: Severity::Error,
                code: "config-error".to_string(),
                message: format!("scanning input: {}", e),
                targets: vec![],
            });
            snap.success = false;
            snap.duration = started.elapsed();
            snap.finished_at = Some(Instant::now());
            return snap;
        }
    };

    let project = parse_project(files);
    snap.diagnostics
        .extend(project.diagnostics.diagnostics.iter().cloned());

    if project.diagnostics.has_errors() {
        snap.success = false;
        snap.duration = started.elapsed();
        snap.finished_at = Some(Instant::now());
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

        let response_files: Vec<GeneratedFile> =
            if let Some(generator_name) = &output_config.generator {
                match generator_name.as_str() {
                    "typescript" => {
                        TypeScriptGenerator::from_options(&options)
                            .generate(&request)
                            .files
                    }
                    "rust" => {
                        RustGenerator::from_options(&options).generate(&request).files
                    }
                    "python" => {
                        PythonGenerator::from_options(&options)
                            .generate(&request)
                            .files
                    }
                    other => {
                        snap.diagnostics.push(PrimateDiagnostic {
                            file: config_path.display().to_string(),
                            line: 1,
                            column: 1,
                            length: None,
                            severity: Severity::Error,
                            code: "config-error".to_string(),
                            message: format!("unknown generator `{}`", other),
                            targets: vec![],
                        });
                        continue;
                    }
                }
            } else {
                continue;
            };

        for file in response_files {
            if let Some(parent) = Path::new(&file.path).parent() {
                if !parent.as_os_str().is_empty() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        snap.diagnostics.push(PrimateDiagnostic {
                            file: parent.display().to_string(),
                            line: 1,
                            column: 1,
                            length: None,
                            severity: Severity::Error,
                            code: "io-error".to_string(),
                            message: format!("creating dir: {}", e),
                            targets: vec![],
                        });
                        continue;
                    }
                }
            }
            if let Err(e) = std::fs::write(&file.path, &file.content) {
                snap.diagnostics.push(PrimateDiagnostic {
                    file: file.path.clone(),
                    line: 1,
                    column: 1,
                    length: None,
                    severity: Severity::Error,
                    code: "io-error".to_string(),
                    message: format!("writing file: {}", e),
                    targets: vec![],
                });
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
    snap.finished_at = Some(Instant::now());
    snap
}
