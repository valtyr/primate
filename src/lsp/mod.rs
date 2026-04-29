//! LSP server implementation for .c.toml files

use crate::diagnostics::{Diagnostic as LibDiagnostic, Diagnostics, Severity as LibSeverity};
use crate::parser::{ast::File as AstFile, parse_source, ConstFile, ParsedProject};
use crate::sourcemap::Sourcemap;
use lsp_server::{Connection, Message, Request, RequestId, Response};
use lsp_types::{
    notification::{DidChangeTextDocument, DidOpenTextDocument, Notification, PublishDiagnostics},
    request::{
        Completion, Formatting, GotoDefinition, HoverRequest, References, Request as LspRequest,
    },
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, CompletionList,
    CompletionResponse, CompletionTextEdit, Diagnostic, DiagnosticSeverity,
    GotoDefinitionResponse, Hover, HoverContents, InitializeParams, InsertTextFormat,
    Location, MarkupContent, MarkupKind, OneOf, Position, PublishDiagnosticsParams,
    Range, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextEdit, Uri,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;

struct ServerState {
    documents: HashMap<Uri, String>,
    sourcemap: Option<Sourcemap>,
    sourcemap_base_path: PathBuf,
    config_name: String,
    config_loaded: bool,
    workspace_folders: Vec<PathBuf>,
    /// Per-file lex+parse cache keyed by canonical path. The stored hash is
    /// over the file's content; on a hit we reuse the AST and per-file
    /// diagnostics rather than re-lexing/re-parsing. The lower pass still
    /// runs every call (it's cheap and depends on the full file set).
    parse_cache: HashMap<PathBuf, CachedParse>,
}

#[derive(Clone)]
struct CachedParse {
    content_hash: u64,
    ast: AstFile,
    diagnostics: Vec<LibDiagnostic>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeneratedPositionsParams {
    text_document: lsp_types::TextDocumentIdentifier,
    position: Position,
}

#[derive(Debug)]
enum GeneratedPositionsRequest {}

impl LspRequest for GeneratedPositionsRequest {
    type Params = GeneratedPositionsParams;
    type Result = Vec<Location>;
    const METHOD: &'static str = "primate/generatedPositions";
}

#[derive(Debug, Serialize, Deserialize)]
struct ResolveSourceLocationParams {
    uri: Uri,
    line: u32,
}

#[derive(Debug)]
enum ResolveSourceLocationRequest {}

impl LspRequest for ResolveSourceLocationRequest {
    type Params = ResolveSourceLocationParams;
    type Result = Option<Location>;
    const METHOD: &'static str = "primate/resolveSourceLocation";
}

/// Run the LSP server
// r[impl cli.lsp]
pub fn run_server(config_path: &Path) -> Result<(), Box<dyn Error>> {
    eprintln!("[LSP] Starting primate LSP server...");

    // Store just the config filename - we'll search for it when documents are opened
    let config_name = config_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("primate.toml")
        .to_string();
    eprintln!("[LSP] Will search for config: {}", config_name);

    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        completion_provider: Some(lsp_types::CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![
                ":".to_string(),
                " ".to_string(),
                "<".to_string(),
                ",".to_string(),
                // Digits auto-fire completion so unit-suffix suggestions
                // appear as soon as the user starts typing a numeric
                // literal — e.g. `duration X = 3` pops up `ms`, `s`, …
                "0".to_string(),
                "1".to_string(),
                "2".to_string(),
                "3".to_string(),
                "4".to_string(),
                "5".to_string(),
                "6".to_string(),
                "7".to_string(),
                "8".to_string(),
                "9".to_string(),
            ]),
            ..Default::default()
        }),
        ..Default::default()
    })?;

    let initialization_params = connection.initialize(server_capabilities)?;
    eprintln!("[LSP] Initialized with params: {:?}", initialization_params);

    // Parse workspace folders from initialize params
    let init_params: InitializeParams = serde_json::from_value(initialization_params.clone())?;
    let workspace_folders: Vec<PathBuf> = init_params
        .workspace_folders
        .unwrap_or_default()
        .iter()
        .filter_map(|f| url::Url::parse(f.uri.as_str()).ok()?.to_file_path().ok())
        .collect();
    eprintln!("[LSP] Workspace folders: {:?}", workspace_folders);

    let mut state = ServerState {
        documents: HashMap::new(),
        sourcemap: None,
        sourcemap_base_path: PathBuf::new(),
        config_name,
        config_loaded: false,
        workspace_folders,
        parse_cache: HashMap::new(),
    };

    main_loop(connection, initialization_params, &mut state)?;
    io_threads.join()?;

    eprintln!("[LSP] Server stopped.");
    Ok(())
}

fn main_loop(
    connection: Connection,
    params: serde_json::Value,
    state: &mut ServerState,
) -> Result<(), Box<dyn Error>> {
    let _params: InitializeParams = serde_json::from_value(params)?;

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }

                eprintln!("[LSP] Received request: {}", req.method);

                let req = match cast_request::<GotoDefinition>(req) {
                    Ok((id, params)) => {
                        eprintln!("[LSP] GotoDefinition request: {:?}", params);
                        // r[impl lsp.goto-definition]
                        let url = params.text_document_position_params.text_document.uri;
                        let position = params.text_document_position_params.position;

                        let result = state.goto_definition(&url, position);
                        let result = serde_json::to_value(&result)?;
                        let resp = Response {
                            id,
                            result: Some(result),
                            error: None,
                        };
                        connection.sender.send(Message::Response(resp))?;
                        continue;
                    }
                    Err(req) => req,
                };

                let req = match cast_request::<Completion>(req) {
                    Ok((id, params)) => {
                        eprintln!("[LSP] Completion request: {:?}", params);
                        let url = params.text_document_position.text_document.uri;
                        let position = params.text_document_position.position;
                        let result = state.completion(&url, position);
                        let result = serde_json::to_value(&result)?;
                        let resp = Response {
                            id,
                            result: Some(result),
                            error: None,
                        };
                        connection.sender.send(Message::Response(resp))?;
                        continue;
                    }
                    Err(req) => req,
                };

                let req = match cast_request::<Formatting>(req) {
                    Ok((id, params)) => {
                        let url = params.text_document.uri;
                        let result = state.formatting(&url);
                        let result = serde_json::to_value(&result)?;
                        let resp = Response {
                            id,
                            result: Some(result),
                            error: None,
                        };
                        connection.sender.send(Message::Response(resp))?;
                        continue;
                    }
                    Err(req) => req,
                };

                let req = match cast_request::<HoverRequest>(req) {
                    Ok((id, params)) => {
                        let url = params.text_document_position_params.text_document.uri;
                        let position = params.text_document_position_params.position;
                        let result = state.hover(&url, position);
                        let result = serde_json::to_value(&result)?;
                        let resp = Response {
                            id,
                            result: Some(result),
                            error: None,
                        };
                        connection.sender.send(Message::Response(resp))?;
                        continue;
                    }
                    Err(req) => req,
                };

                let req = match cast_request::<References>(req) {
                    Ok((id, params)) => {
                        let url = params.text_document_position.text_document.uri;
                        let position = params.text_document_position.position;
                        let result = state.references(&url, position);
                        let result = serde_json::to_value(&result)?;
                        let resp = Response {
                            id,
                            result: Some(result),
                            error: None,
                        };
                        connection.sender.send(Message::Response(resp))?;
                        continue;
                    }
                    Err(req) => req,
                };

                // Custom Requests
                let req = match cast_request::<GeneratedPositionsRequest>(req) {
                    Ok((id, params)) => {
                        let result =
                            state.generated_positions(&params.text_document.uri, params.position);
                        let result = serde_json::to_value(&result)?;
                        let resp = Response {
                            id,
                            result: Some(result),
                            error: None,
                        };
                        connection.sender.send(Message::Response(resp))?;
                        continue;
                    }
                    Err(req) => req,
                };

                match cast_request::<ResolveSourceLocationRequest>(req) {
                    Ok((id, params)) => {
                        let result = state.resolve_source_location(&params.uri, params.line);
                        let result = serde_json::to_value(&result)?;
                        let resp = Response {
                            id,
                            result: Some(result),
                            error: None,
                        };
                        connection.sender.send(Message::Response(resp))?;
                        continue;
                    }
                    Err(_req) => {
                        // unhandled
                    }
                };
            }
            Message::Response(_resp) => {}
            Message::Notification(not) => match not.method.as_str() {
                DidOpenTextDocument::METHOD => {
                    let params: lsp_types::DidOpenTextDocumentParams =
                        serde_json::from_value(not.params)?;
                    eprintln!(
                        "[LSP] DidOpenTextDocument: {}",
                        params.text_document.uri.as_str()
                    );

                    // Try to load config/sourcemap if not yet loaded
                    if !state.config_loaded {
                        state.try_load_config_from_document(&params.text_document.uri);
                    }

                    state
                        .documents
                        .insert(params.text_document.uri.clone(), params.text_document.text);
                    state.publish_diagnostics(&connection, &params.text_document.uri)?;
                }
                DidChangeTextDocument::METHOD => {
                    let params: lsp_types::DidChangeTextDocumentParams =
                        serde_json::from_value(not.params)?;
                    eprintln!(
                        "[LSP] DidChangeTextDocument: {}",
                        params.text_document.uri.as_str()
                    );
                    if let Some(change) = params.content_changes.into_iter().last() {
                        state
                            .documents
                            .insert(params.text_document.uri.clone(), change.text);
                        state.publish_diagnostics(&connection, &params.text_document.uri)?;
                    }
                }
                _ => {}
            },
        }
    }
    Ok(())
}

impl ServerState {
    /// Find all config files in workspace folders (immediate children only)
    fn find_configs_in_workspaces(&self) -> Vec<PathBuf> {
        let mut configs = Vec::new();
        for folder in &self.workspace_folders {
            // Check workspace_root/primate.toml
            let config_path = folder.join(&self.config_name);
            if config_path.exists() {
                eprintln!("[LSP] Found config at workspace root: {}", config_path.display());
                configs.push(config_path);
            }
            // Check workspace_root/*/primate.toml (one level deep)
            // Handles: packages/constants/primate.toml, apps/foo/primate.toml
            if let Ok(entries) = std::fs::read_dir(folder) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let sub_config = entry.path().join(&self.config_name);
                        if sub_config.exists() {
                            eprintln!("[LSP] Found config in subdirectory: {}", sub_config.display());
                            configs.push(sub_config);
                        }
                    }
                }
            }
        }
        configs
    }

    /// Search for config files - first in workspace folders, then upward from document
    fn try_load_config_from_document(&mut self, uri: &Uri) {
        // First, try to find configs in workspace folders
        let workspace_configs = self.find_configs_in_workspaces();
        if !workspace_configs.is_empty() {
            // Load the first config found (TODO: support multiple sourcemaps for multi-package monorepos)
            for config_path in &workspace_configs {
                self.load_sourcemap(config_path);
            }
            self.config_loaded = true;
            return;
        }

        // Fallback: parse URI to file path and search upward
        let doc_path = match url::Url::parse(uri.as_str())
            .ok()
            .and_then(|u| u.to_file_path().ok())
        {
            Some(p) => p,
            None => {
                eprintln!("[LSP] Could not parse document URI: {}", uri.as_str());
                return;
            }
        };

        // Search upward for config file
        let mut search_dir = doc_path.parent();
        while let Some(dir) = search_dir {
            let config_path = dir.join(&self.config_name);
            if config_path.exists() {
                eprintln!("[LSP] Found config at: {}", config_path.display());
                self.load_sourcemap(&config_path);
                self.config_loaded = true;
                return;
            }
            search_dir = dir.parent();
        }

        eprintln!(
            "[LSP] No {} found in workspace folders or parent directories of {}",
            self.config_name,
            doc_path.display()
        );
    }

    fn load_sourcemap(&mut self, config_path: &Path) {
        // Try to load config to get sourcemap path (may have override)
        let sourcemap_path = match crate::config::Config::load(config_path) {
            Ok(config) => {
                let path = config.sourcemap_path(config_path);
                eprintln!("[LSP] Config loaded, sourcemap path: {}", path.display());
                path
            }
            Err(e) => {
                // If config can't be loaded, fall back to default location
                eprintln!(
                    "[LSP] Could not load config: {}, using default sourcemap location",
                    e
                );
                let config_dir = config_path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                config_dir.join("primate.sourcemap.json")
            }
        };

        self.sourcemap_base_path = sourcemap_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        eprintln!(
            "[LSP] Loading sourcemap from: {}, base: {}",
            sourcemap_path.display(),
            self.sourcemap_base_path.display()
        );

        if let Ok(content) = std::fs::read_to_string(&sourcemap_path) {
            if let Ok(sourcemap) = serde_json::from_str::<Sourcemap>(&content) {
                eprintln!(
                    "[LSP] Loaded sourcemap with {} entries",
                    sourcemap.entries.len()
                );
                self.sourcemap = Some(sourcemap);
            } else {
                eprintln!("[LSP] Failed to parse sourcemap");
            }
        } else {
            eprintln!("[LSP] No sourcemap found at {}", sourcemap_path.display());
        }
    }

    fn resolve_path_to_uri(&self, path_str: &str) -> Option<Uri> {
        let path = Path::new(path_str);
        // If absolute, trust it.
        // If relative, join with sourcemap_base_path.
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            // Check for leading ./ or just join directly?
            // Path::join handles relative paths fine.
            // But if path starts with ., strip it to be clean
            let clean_path = if path.starts_with(".") {
                path.strip_prefix(".").unwrap_or(path)
            } else {
                path
            };
            self.sourcemap_base_path.join(clean_path)
        };

        // Canonicalize to resolve symlinks and ../
        // Note: canonicalize FAILS if file does not exist.
        match std::fs::canonicalize(&abs_path) {
            Ok(canon) => {
                if let Ok(url) = url::Url::from_file_path(canon) {
                    if let Ok(uri) = Uri::from_str(url.as_str()) {
                        return Some(uri);
                    }
                }
            }
            Err(_) => {
                // Fallback: try to construct URL from non-canonical path
                if let Ok(url) = url::Url::from_file_path(&abs_path) {
                    if let Ok(uri) = Uri::from_str(url.as_str()) {
                        return Some(uri);
                    }
                }
            }
        }
        None
    }

    fn publish_diagnostics(
        &mut self,
        connection: &Connection,
        uri: &Uri,
    ) -> Result<(), Box<dyn Error>> {
        let content = self.documents.get(uri).cloned().unwrap_or_default();
        let path = url::Url::parse(uri.as_str())
            .ok()
            .and_then(|u| u.to_file_path().ok())
            .unwrap_or_default();

        let mut lsp_diagnostics = Vec::new();

        if path.file_name().and_then(|s| s.to_str()) == Some("primate.toml") {
            let lib_diagnostics =
                crate::config::Config::check(&content, path.to_str().unwrap_or("primate.toml"));
            for diag in lib_diagnostics.diagnostics {
                lsp_diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position {
                            line: diag.line.saturating_sub(1),
                            character: diag.column.saturating_sub(1),
                        },
                        end: Position {
                            line: diag.line.saturating_sub(1),
                            character: if let Some(len) = diag.length {
                                diag.column.saturating_sub(1) + len
                            } else {
                                100 // fallback
                            },
                        },
                    },
                    severity: Some(match diag.severity {
                        LibSeverity::Error => DiagnosticSeverity::ERROR,
                        LibSeverity::Warning => DiagnosticSeverity::WARNING,
                        LibSeverity::Info => DiagnosticSeverity::INFORMATION,
                    }),
                    code: Some(lsp_types::NumberOrString::String(diag.code)),
                    message: diag.message,
                    ..Default::default()
                });
            }
        } else {
            // r[impl lsp.diagnostics]
            // Parse the full workspace so cross-file type references resolve.
            // Filter the diagnostics to those that belong to the current file.
            let (project, current_canon) = self.parse_workspace(uri, &content);
            let current_path_str = current_canon
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| path.display().to_string());
            for diag in project.diagnostics.diagnostics {
                if diag.file != current_path_str {
                    continue;
                }
                lsp_diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position {
                            line: diag.line.saturating_sub(1),
                            character: diag.column.saturating_sub(1),
                        },
                        end: Position {
                            line: diag.line.saturating_sub(1),
                            character: if let Some(len) = diag.length {
                                diag.column.saturating_sub(1) + len
                            } else {
                                100
                            },
                        },
                    },
                    severity: Some(match diag.severity {
                        LibSeverity::Error => DiagnosticSeverity::ERROR,
                        LibSeverity::Warning => DiagnosticSeverity::WARNING,
                        LibSeverity::Info => DiagnosticSeverity::INFORMATION,
                    }),
                    code: Some(lsp_types::NumberOrString::String(diag.code)),
                    message: diag.message,
                    ..Default::default()
                });
            }
        }

        let params = PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: lsp_diagnostics,
            version: None,
        };
        let not = lsp_server::Notification::new(PublishDiagnostics::METHOD.to_string(), params);
        connection.sender.send(Message::Notification(not))?;

        Ok(())
    }

    /// Build a workspace-wide list of `.prim` files, replacing the on-disk
    /// content of `current_uri` (if it appears in the workspace) with the live
    /// buffer's content. The returned current-file path is canonicalized, so
    /// callers can match it against the `file` field of diagnostics or
    /// `source.file` of IR types.
    ///
    /// Re-walks and re-parses on every call. Could be cached if it shows up
    /// in profiles for large projects.
    fn collect_workspace_files_with_buffer(
        &self,
        current_uri: &Uri,
        current_content: &str,
    ) -> (Vec<ConstFile>, Option<PathBuf>) {
        let current_path = url::Url::parse(current_uri.as_str())
            .ok()
            .and_then(|u| u.to_file_path().ok());
        let current_canon = current_path
            .as_ref()
            .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()));

        let mut files: Vec<ConstFile> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut current_included = false;

        // Resolve namespaces against each project's `input` directory rather
        // than the workspace folder. A workspace can host multiple
        // primate.toml files (monorepo with several primate projects); each
        // is its own root, with its own `input`. We discover each project
        // independently and dedup by canonical path so a file isn't pulled
        // in twice.
        let configs = self.find_configs_in_workspaces();
        let mut input_dirs: Vec<PathBuf> = Vec::new();

        for config_path in &configs {
            let input_dir = match crate::config::Config::load(config_path) {
                Ok(config) => {
                    let config_dir = config_path
                        .parent()
                        .filter(|p| !p.as_os_str().is_empty())
                        .unwrap_or_else(|| Path::new("."));
                    if config.input.is_absolute() {
                        config.input
                    } else {
                        config_dir.join(&config.input)
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[LSP] Could not load {}: {} — falling back to workspace folder",
                        config_path.display(),
                        e,
                    );
                    continue;
                }
            };
            input_dirs.push(input_dir);
        }

        // If no configs are loadable, treat the workspace folders themselves
        // as input dirs. This lets the LSP work in a directory that has
        // .prim files but no primate.toml yet (e.g. fresh checkout).
        if input_dirs.is_empty() {
            input_dirs.extend(self.workspace_folders.iter().cloned());
        }

        for input_dir in &input_dirs {
            match crate::parser::discover_files(input_dir) {
                Ok(discovered) => {
                    for mut f in discovered {
                        let canon = std::fs::canonicalize(&f.path)
                            .unwrap_or_else(|_| f.path.clone());
                        if !seen.insert(canon.clone()) {
                            continue;
                        }
                        if let Some(ref cur_canon) = current_canon {
                            if &canon == cur_canon {
                                f.content = current_content.to_string();
                                current_included = true;
                            }
                        }
                        // Use the canonical path so diagnostic matching is
                        // stable regardless of how the file was discovered.
                        f.path = canon;
                        files.push(f);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[LSP] discover_files failed for {}: {}",
                        input_dir.display(),
                        e,
                    );
                }
            }
        }

        // If the current document isn't inside any project's `input` (or
        // there are no projects), include it as a singleton with a single-
        // segment namespace derived from the file stem so we can still
        // report diagnostics and resolve its own types.
        if !current_included {
            if let Some(ref path) = current_path {
                let canon = current_canon.clone().unwrap_or_else(|| path.clone());
                let namespace = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("lsp")
                    .to_string();
                files.push(ConstFile {
                    path: canon,
                    namespace,
                    content: current_content.to_string(),
                });
            }
        }

        (files, current_canon)
    }

    /// Parse the entire workspace plus the live buffer for `current_uri` as a
    /// single project. Cross-file type references resolve correctly here.
    ///
    /// Lex+parse for each file is cached by `parse_cache` keyed on canonical
    /// path: a file whose content hash matches the cached entry skips
    /// re-parsing. The lower pass runs every call regardless (cheap, and it
    /// depends on the full file set).
    fn parse_workspace(
        &mut self,
        current_uri: &Uri,
        current_content: &str,
    ) -> (ParsedProject, Option<PathBuf>) {
        let (files, current_canon) =
            self.collect_workspace_files_with_buffer(current_uri, current_content);

        let mut diagnostics = Diagnostics::new();
        let mut parsed_files: Vec<crate::parser::ParsedFile> = Vec::with_capacity(files.len());

        for file in files {
            let hash = hash_str(&file.content);
            let cached = match self.parse_cache.get(&file.path) {
                Some(c) if c.content_hash == hash => Some(c.clone()),
                _ => None,
            };
            let (ast, file_diags) = if let Some(c) = cached {
                (c.ast, c.diagnostics)
            } else {
                let (ast, diags) = parse_source(&file.content, &file.path);
                let file_diags: Vec<LibDiagnostic> = diags.diagnostics;
                self.parse_cache.insert(
                    file.path.clone(),
                    CachedParse {
                        content_hash: hash,
                        ast: ast.clone(),
                        diagnostics: file_diags.clone(),
                    },
                );
                (ast, file_diags)
            };

            for d in file_diags {
                diagnostics.add(d);
            }
            parsed_files.push(crate::parser::ParsedFile {
                path: file.path,
                default_namespace: file.namespace,
                ast,
                source_text: file.content,
            });
        }

        let resolved = crate::parser::lower::lower(parsed_files);
        for d in resolved.diagnostics.diagnostics {
            diagnostics.add(d);
        }

        (
            ParsedProject {
                modules: resolved.modules,
                enums: resolved.enums,
                aliases: resolved.aliases,
                diagnostics,
            },
            current_canon,
        )
    }

    fn goto_definition(&mut self, uri: &Uri, position: Position) -> Option<GotoDefinitionResponse> {
        let content = self.documents.get(uri)?.clone();
        let path = qualified_path_at(&content, position)?;
        let segments: Vec<&str> = path.split("::").collect();
        let last = *segments.last()?;
        let qualifier: Option<String> = if segments.len() > 1 {
            Some(segments[..segments.len() - 1].join("::"))
        } else {
            None
        };

        let (project, _current_canon) = self.parse_workspace(uri, &content);

        for e in &project.enums {
            let ns_match = match &qualifier {
                Some(q) => &e.namespace == q,
                None => true,
            };
            if e.name == last && ns_match {
                if let Some(loc) = source_location_to_lsp(&e.source) {
                    return Some(GotoDefinitionResponse::Scalar(loc));
                }
            }
        }
        for a in &project.aliases {
            let ns_match = match &qualifier {
                Some(q) => &a.namespace == q,
                None => true,
            };
            if a.name == last && ns_match {
                if let Some(loc) = source_location_to_lsp(&a.source) {
                    return Some(GotoDefinitionResponse::Scalar(loc));
                }
            }
        }
        None
    }

    /// Find every reference to the type at `position` across the workspace.
    fn references(&mut self, uri: &Uri, position: Position) -> Option<Vec<Location>> {
        let content = self.documents.get(uri)?.clone();
        let target = qualified_path_at(&content, position)?;
        let segments: Vec<&str> = target.split("::").collect();
        let target_name = (*segments.last()?).to_string();
        let target_namespace: Option<String> = if segments.len() > 1 {
            Some(segments[..segments.len() - 1].join("::"))
        } else {
            None
        };

        let (project, _current_canon) = self.parse_workspace(uri, &content);

        // Determine the canonical (namespace, name) of the target. If the user
        // hovered an unqualified reference, look the name up to find which
        // namespace it lives in.
        let resolved_namespace: Option<String> = match &target_namespace {
            Some(ns) => Some(ns.clone()),
            None => project
                .enums
                .iter()
                .find(|e| e.name == target_name)
                .map(|e| e.namespace.clone())
                .or_else(|| {
                    project
                        .aliases
                        .iter()
                        .find(|a| a.name == target_name)
                        .map(|a| a.namespace.clone())
                }),
        };
        let resolved_namespace = resolved_namespace?;

        let mut locations: Vec<Location> = Vec::new();

        // Re-parse each workspace file individually to access its tokens. We
        // already have ConstFile content in memory via collect_workspace_files_with_buffer.
        let (files, _current_canon) = self.collect_workspace_files_with_buffer(uri, &content);

        // Pre-compute each file's effective namespace via parse_project so
        // unqualified references resolve correctly. We already have `project`
        // but its modules carry the namespace — index it by source file.
        let mut file_namespace: HashMap<String, String> = HashMap::new();
        for module in &project.modules {
            file_namespace.insert(module.source_file.clone(), module.namespace.clone());
        }

        for file in &files {
            let file_path_str = file.path.display().to_string();
            // Effective namespace for unqualified resolution.
            let eff_ns = file_namespace
                .get(&file_path_str)
                .cloned()
                .unwrap_or_else(|| file.namespace.clone());

            // Per-file imports: map a bare imported name to the namespace it
            // came from. Lets us match unqualified references in files that
            // brought the type into scope via `use`.
            let imports: HashMap<String, String> = {
                let (ast, _) = parse_source(&file.content, &file.path);
                let mut m = HashMap::new();
                for item in &ast.items {
                    if let crate::parser::ast::Item::Use(u) = item {
                        let import_ns = u.path.join("::");
                        for it in &u.items {
                            m.insert(it.name.clone(), import_ns.clone());
                        }
                    }
                }
                m
            };

            let (tokens, _lex_errs) = crate::parser::lexer::Lexer::new(&file.content).lex_all();
            // Scan for path sequences: Ident (ColonColon Ident)*.
            use crate::parser::lexer::Tok;
            let mut i = 0;
            while i < tokens.len() {
                if let Tok::Ident(_) = &tokens[i].tok {
                    // Walk a path.
                    let path_start = i;
                    let mut path_segs: Vec<String> = Vec::new();
                    if let Tok::Ident(s) = &tokens[i].tok {
                        path_segs.push(s.clone());
                    }
                    let mut j = i + 1;
                    while j + 1 < tokens.len()
                        && matches!(tokens[j].tok, Tok::ColonColon)
                    {
                        if let Tok::Ident(s) = &tokens[j + 1].tok {
                            path_segs.push(s.clone());
                            j += 2;
                        } else {
                            break;
                        }
                    }

                    // Does this path resolve to (resolved_namespace, target_name)?
                    let last = path_segs.last().cloned().unwrap_or_default();
                    if last == target_name {
                        let matches = if path_segs.len() > 1 {
                            let ns = path_segs[..path_segs.len() - 1].join("::");
                            ns == resolved_namespace
                        } else {
                            // Unqualified: match if (a) the file's effective
                            // namespace equals the target namespace, or (b) the
                            // file imports this name from the target namespace.
                            eff_ns == resolved_namespace
                                || imports
                                    .get(&last)
                                    .map(|ns| ns == &resolved_namespace)
                                    .unwrap_or(false)
                        };
                        if matches {
                            let span = tokens[path_start].span;
                            let end_span = tokens[j.saturating_sub(1).max(path_start)].span;
                            if let Some(uri) = path_to_uri(&file.path) {
                                locations.push(Location {
                                    uri,
                                    range: Range {
                                        start: Position {
                                            line: span.line.saturating_sub(1),
                                            character: span.column.saturating_sub(1),
                                        },
                                        end: Position {
                                            line: end_span.line.saturating_sub(1),
                                            character: end_span
                                                .column
                                                .saturating_sub(1)
                                                + end_span.len(),
                                        },
                                    },
                                });
                            }
                        }
                    }

                    i = j.max(i + 1);
                } else {
                    i += 1;
                }
            }
        }

        Some(locations)
    }
    fn hover(&mut self, uri: &Uri, position: Position) -> Option<Hover> {
        let content = self.documents.get(uri)?.clone();
        let path = qualified_path_at(&content, position)?;
        let segments: Vec<&str> = path.split("::").collect();
        let last = *segments.last()?;
        let qualifier: Option<String> = if segments.len() > 1 {
            Some(segments[..segments.len() - 1].join("::"))
        } else {
            None
        };

        // Built-in primitive descriptions (no doc lookup needed).
        if qualifier.is_none() {
            if let Some(doc) = primitive_hover(last) {
                return Some(make_hover(doc));
            }
            if let Some(doc) = container_hover(last) {
                return Some(make_hover(doc));
            }
        }

        // Look the type up in the workspace project.
        let (project, _current_canon) = self.parse_workspace(uri, &content);

        for e in &project.enums {
            let ns_match = match &qualifier {
                Some(q) => &e.namespace == q,
                None => true,
            };
            if e.name == last && ns_match {
                let mut md = String::new();
                md.push_str("```primate\n");
                if e.namespace.is_empty() {
                    md.push_str(&format!("enum {}", e.name));
                } else {
                    md.push_str(&format!("enum {}::{}", e.namespace, e.name));
                }
                md.push_str(&format!(" : {}", e.backing_type));
                md.push_str("\n```");
                if let Some(doc) = &e.doc {
                    md.push_str("\n\n");
                    md.push_str(doc);
                }
                if !e.variants.is_empty() {
                    md.push_str("\n\n**Variants:** ");
                    let names: Vec<String> =
                        e.variants.iter().map(|v| v.name.clone()).collect();
                    md.push_str(&names.join(", "));
                }
                return Some(make_hover(md));
            }
        }

        for a in &project.aliases {
            let ns_match = match &qualifier {
                Some(q) => &a.namespace == q,
                None => true,
            };
            if a.name == last && ns_match {
                let mut md = String::new();
                md.push_str("```primate\n");
                if a.namespace.is_empty() {
                    md.push_str(&format!("type {} = {}", a.name, format_type(&a.target)));
                } else {
                    md.push_str(&format!(
                        "type {}::{} = {}",
                        a.namespace,
                        a.name,
                        format_type(&a.target)
                    ));
                }
                md.push_str("\n```");
                if let Some(doc) = &a.doc {
                    md.push_str("\n\n");
                    md.push_str(doc);
                }
                return Some(make_hover(md));
            }
        }

        None
    }

    fn completion(&mut self, uri: &Uri, position: Position) -> Option<CompletionResponse> {
        let content = self.documents.get(uri)?.clone();

        // Compute byte offset of the cursor and grab the current line up to it.
        let line_prefix = line_prefix_at(&content, position);

        // If a `//` (any kind: `//`, `///`, `//!`) appears in the line prefix,
        // the cursor is past it — we're inside a comment and should not offer
        // type/keyword completions.
        if line_prefix.contains("//") {
            return Some(CompletionResponse::Array(Vec::new()));
        }

        let ctx = detect_completion_context(&line_prefix);

        // Parse the whole workspace plus the live buffer once. We split the
        // resolved types into "current file" and "elsewhere" by comparing each
        // type's `source.file` to the current file's canonical path.
        let (project, current_canon) = self.parse_workspace(uri, &content);
        let current_path_str = current_canon.as_ref().map(|p| p.display().to_string());

        let mut local_enums: Vec<(String, String)> = Vec::new();
        let mut local_aliases: Vec<(String, String)> = Vec::new();
        let mut workspace_enums: Vec<(String, String, String)> = Vec::new();
        let mut workspace_aliases: Vec<(String, String, String)> = Vec::new();
        for e in &project.enums {
            if Some(&e.source.file) == current_path_str.as_ref() {
                local_enums.push((e.name.clone(), "enum (this file)".to_string()));
            } else {
                workspace_enums.push((e.name.clone(), e.namespace.clone(), "enum".to_string()));
            }
        }
        for a in &project.aliases {
            if Some(&a.source.file) == current_path_str.as_ref() {
                local_aliases.push((a.name.clone(), "alias (this file)".to_string()));
            } else {
                workspace_aliases.push((a.name.clone(), a.namespace.clone(), "alias".to_string()));
            }
        }

        let mut completions = Vec::new();
        match ctx {
            CompletionContext::DeclStart => {
                push_decl_start_keywords(&mut completions);
                push_primitive_types(&mut completions);
                push_container_constructors(&mut completions);
                push_local_user_types(&mut completions, &local_enums, &local_aliases);
                push_workspace_user_types(&mut completions, &workspace_enums, &workspace_aliases);
            }
            CompletionContext::TypePosition => {
                push_primitive_types(&mut completions);
                push_container_constructors(&mut completions);
                push_local_user_types(&mut completions, &local_enums, &local_aliases);
                push_workspace_user_types(&mut completions, &workspace_enums, &workspace_aliases);
            }
            CompletionContext::ValuePosition => {
                // Value completions are driven by the declared type of the
                // constant being assigned. Booleans get `true`/`false`,
                // optionals get `none` plus completions for the inner type,
                // enums get their variants. Anything else gets no
                // completions — there's no useful suggestion for arbitrary
                // numbers, strings, durations, or container literals.
                let declared = extract_declared_type_from_prefix(&line_prefix);
                if let Some(ref declared) = declared {
                    push_value_completions_for_type(
                        declared,
                        &project,
                        &mut completions,
                    );

                    // Unit-suffix completions: when the line prefix ends with
                    // a digit (and optional in-progress alphabetic suffix),
                    // offer the suffixes that apply to the declared type.
                    // The textEdit replaces just the alphabetic part so
                    // selecting `min` after typing `30m` yields `30min`, not
                    // `30mmin` or `min`.
                    if let Some((range, digits)) = pending_suffix_state(&line_prefix, position) {
                        push_unit_suffix_completions(declared, range, &digits, &mut completions);
                    }
                }
            }
        }

        // Return as an incomplete list so the editor re-requests on every
        // keystroke instead of filtering the existing items locally. This
        // matters for unit-suffix completions: the labels embed the digits
        // the user has already typed (`30ms`), so when they type one more
        // digit the editor's prefix filter would drop every item — we need
        // a fresh list with `300ms`, `300s`, …
        Some(CompletionResponse::List(CompletionList {
            is_incomplete: true,
            items: completions,
        }))
    }

    fn formatting(&self, uri: &Uri) -> Option<Vec<TextEdit>> {
        let content = self.documents.get(uri)?.clone();
        let formatted = match crate::formatter::format_source(&content) {
            Ok(s) => s,
            Err(_) => return None,
        };
        if formatted == content {
            return Some(vec![]);
        }
        // Replace the whole document.
        let last_line = content.lines().count() as u32;
        let last_line_len = content
            .lines()
            .last()
            .map(|l| l.chars().count() as u32)
            .unwrap_or(0);
        Some(vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: last_line,
                    character: last_line_len,
                },
            },
            new_text: formatted,
        }])
    }

    fn generated_positions(&self, uri: &Uri, position: Position) -> Vec<Location> {
        eprintln!(
            "[LSP] generated_positions called: uri={}, line={}",
            uri.as_str(),
            position.line
        );

        let path = url::Url::parse(uri.as_str())
            .ok()
            .and_then(|u| u.to_file_path().ok())
            .unwrap_or_default();

        let target_line = position.line + 1; // 1-based in sourcemap
        let mut locations = Vec::new();

        if let Some(sourcemap) = &self.sourcemap {
            for entry in &sourcemap.entries {
                // Strip leading ./ from sourcemap paths for reliable matching
                let entry_source = entry
                    .source_file
                    .strip_prefix("./")
                    .unwrap_or(&entry.source_file);
                let entry_source_path = Path::new(entry_source);

                eprintln!(
                    "[LSP]   checking: {} vs {}",
                    path.display(),
                    entry_source_path.display()
                );

                // Check if paths match (editor sends absolute, sourcemap has relative)
                if path.ends_with(entry_source_path) && entry.source_line == target_line {
                    // Use resolve_path_to_uri to get absolute URI for output file
                    if let Some(lsp_uri) = self.resolve_path_to_uri(&entry.output_file) {
                        // Use output_column for precise symbol position (1-based to 0-based)
                        let output_col = if entry.output_column > 0 {
                            entry.output_column - 1
                        } else {
                            0
                        };
                        eprintln!(
                            "[LSP]   MATCH! output={} line={} col={}",
                            entry.output_file, entry.output_line, output_col
                        );
                        locations.push(Location {
                            uri: lsp_uri,
                            range: Range {
                                start: Position {
                                    line: entry.output_line - 1, // 0-based
                                    character: output_col,
                                },
                                end: Position {
                                    line: entry.output_line - 1,
                                    character: output_col,
                                },
                            },
                        });
                    }
                }
            }
        } else {
            eprintln!("[LSP]   no sourcemap loaded");
        }

        eprintln!(
            "[LSP] generated_positions returning {} locations",
            locations.len()
        );
        locations
    }

    fn resolve_source_location(&self, uri: &Uri, line: u32) -> Option<Location> {
        eprintln!(
            "[LSP] resolve_source_location called: uri={}, line={}",
            uri.as_str(),
            line
        );

        let path = url::Url::parse(uri.as_str())
            .ok()
            .and_then(|u| u.to_file_path().ok());

        let path = match path {
            Some(p) => p,
            None => {
                eprintln!("[LSP]   failed to parse URI to file path");
                return None;
            }
        };

        let target_line = line + 1; // 1-based in sourcemap

        if let Some(sourcemap) = &self.sourcemap {
            for entry in &sourcemap.entries {
                // Strip leading ./ from sourcemap paths for reliable matching
                let entry_output = entry
                    .output_file
                    .strip_prefix("./")
                    .unwrap_or(&entry.output_file);
                let entry_output_path = Path::new(entry_output);

                eprintln!(
                    "[LSP]   checking: {} vs {} (line {} vs {})",
                    path.display(),
                    entry_output_path.display(),
                    target_line,
                    entry.output_line
                );

                // Check if paths match (editor sends absolute, sourcemap has relative)
                if path.ends_with(entry_output_path) && entry.output_line == target_line {
                    // Use resolve_path_to_uri to get absolute URI for source file
                    if let Some(lsp_uri) = self.resolve_path_to_uri(&entry.source_file) {
                        eprintln!(
                            "[LSP]   MATCH! source={} line={}",
                            entry.source_file, entry.source_line
                        );
                        return Some(Location {
                            uri: lsp_uri,
                            range: Range {
                                start: Position {
                                    line: entry.source_line - 1,
                                    character: entry.source_column - 1,
                                },
                                end: Position {
                                    line: entry.source_line - 1,
                                    character: entry.source_column - 1,
                                },
                            },
                        });
                    } else {
                        eprintln!(
                            "[LSP]   path match but failed to resolve source URI: {}",
                            entry.source_file
                        );
                    }
                }
            }
            eprintln!("[LSP]   no matching entry found in sourcemap");
        } else {
            eprintln!("[LSP]   no sourcemap loaded");
        }

        eprintln!("[LSP] resolve_source_location returning None");
        None
    }
}

// ---------- Completion context detection & item helpers ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionContext {
    /// Beginning of a declaration line (no leading content yet, or only a partial
    /// keyword/type ident). Offers `namespace`/`enum`/`type` plus all type names.
    DeclStart,
    /// Inside a type expression: inside `<...>`, after `type X = `, or anywhere
    /// the parser would expect a type. Offers types but not declaration keywords.
    TypePosition,
    /// After `=` in a `T NAME = ` declaration (not a `type X = `). Offers
    /// `true`/`false`/`none`.
    ValuePosition,
}

/// Get the substring of `content` from the start of `position`'s line up to
/// the cursor column. Falls back to the whole document if `position` is past
/// the end.
fn line_prefix_at(content: &str, position: Position) -> String {
    // Find start byte of the requested line.
    let mut line_idx: u32 = 0;
    let mut line_start: usize = 0;
    for (i, b) in content.bytes().enumerate() {
        if line_idx == position.line {
            break;
        }
        if b == b'\n' {
            line_idx += 1;
            line_start = i + 1;
        }
    }
    if line_idx < position.line {
        return String::new();
    }
    let line_end = content[line_start..]
        .find('\n')
        .map(|n| line_start + n)
        .unwrap_or(content.len());
    let line = &content[line_start..line_end];
    // `position.character` is in UTF-16 code units per LSP spec, but for ASCII-only
    // identifiers it matches char index. Approximate with chars().
    let prefix: String = line.chars().take(position.character as usize).collect();
    prefix
}

/// Pull the declared type expression out of a line prefix that looks like
/// `<type> <NAME> = <cursor>`. Returns the literal type expression — including
/// generics and `?` sugar — so a downstream classifier can interpret it.
/// Returns `None` if the line doesn't fit `<type> <NAME> =` shape at all
/// (e.g. multi-line declarations the cursor can't see).
fn extract_declared_type_from_prefix(line_prefix: &str) -> Option<String> {
    let lhs = line_prefix.split('=').next()?.trim();
    if lhs.is_empty() {
        return None;
    }
    // The constant name is the last whitespace-separated token (it can't
    // contain `<`, `>`, `[`, `]`, `,`, `?` — only identifier chars). Walk
    // backwards through the lhs to find where the name starts.
    let bytes = lhs.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let name_end = end;
    while end > 0 {
        let c = bytes[end - 1];
        if c.is_ascii_alphanumeric() || c == b'_' {
            end -= 1;
        } else {
            break;
        }
    }
    let name_start = end;
    if name_start == name_end {
        return None;
    }
    // Everything before the name is the type expression.
    let type_text = lhs[..name_start].trim();
    if type_text.is_empty() {
        return None;
    }
    Some(type_text.to_string())
}

/// Push value-position completions appropriate for `type_text`. Recurses
/// through optional/sugared-optional to also offer `none` and inner-type
/// suggestions.
fn push_value_completions_for_type(
    type_text: &str,
    project: &ParsedProject,
    completions: &mut Vec<CompletionItem>,
) {
    let trimmed = type_text.trim();

    // Sugared optional: `T?`. Recurse on the inner type and add `none`.
    if let Some(inner) = trimmed.strip_suffix('?') {
        completions.push(CompletionItem {
            label: "none".to_string(),
            kind: Some(CompletionItemKind::VALUE),
            sort_text: Some("0_none".to_string()),
            ..Default::default()
        });
        push_value_completions_for_type(inner, project, completions);
        return;
    }

    // Generic optional: `optional<T>`. Same as above.
    if let Some(rest) = trimmed.strip_prefix("optional<") {
        if let Some(inner) = rest.strip_suffix('>') {
            completions.push(CompletionItem {
                label: "none".to_string(),
                kind: Some(CompletionItemKind::VALUE),
                sort_text: Some("0_none".to_string()),
                ..Default::default()
            });
            push_value_completions_for_type(inner, project, completions);
        }
        return;
    }

    // Bool: only `true`/`false` make sense.
    if trimmed == "bool" {
        for kw in &["true", "false"] {
            completions.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::VALUE),
                sort_text: Some(format!("0_{}", kw)),
                ..Default::default()
            });
        }
        return;
    }

    // Enum (possibly qualified): suggest variants. Also follow alias chains
    // — if the user wrote `Severity LEVEL = ` and `Severity` is `type Severity = LogLevel`,
    // the variants of `LogLevel` should still show up.
    let leaf = trimmed.rsplit("::").next().unwrap_or(trimmed);
    let mut visited: HashSet<String> = HashSet::new();
    let mut current_name = leaf.to_string();
    loop {
        if !visited.insert(current_name.clone()) {
            break;
        }
        let enum_match = project.enums.iter().find(|e| e.name == current_name);
        if let Some(enum_def) = enum_match {
            for variant in &enum_def.variants {
                completions.push(CompletionItem {
                    label: variant.name.clone(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    sort_text: Some(format!("0_{}", variant.name)),
                    detail: Some(format!("variant of {}", enum_def.name)),
                    ..Default::default()
                });
            }
            return;
        }
        // Not an enum — see if it's an alias and chase the target.
        let alias_match = project.aliases.iter().find(|a| a.name == current_name);
        match alias_match {
            Some(alias) => match &alias.target {
                crate::types::Type::Enum { name, .. } | crate::types::Type::Alias { name, .. } => {
                    current_name = name.clone();
                }
                _ => break,
            },
            None => break,
        }
    }

    // Anything else — no useful suggestions. Returning here means the user
    // sees no menu rather than misleading literals like `true`/`none` for
    // an integer-typed constant.
}

/// If the line prefix ends with a numeric literal (digits, plus optional
/// in-progress alphabetic suffix), returns information needed to insert a
/// suffix completion: the range covering the whole `<digits><alpha>` tail,
/// and the digit run as a string.
///
/// Both pieces matter for editor compatibility:
/// - The range must include the digits so editors that derive the current
///   "word" from the textEdit range see something starting with a digit.
/// - The digit string lets us build a `filter_text` like `"30ms"` so prefix
///   matching against what the user has typed (`"30"`, `"30m"`) succeeds.
fn pending_suffix_state(line_prefix: &str, cursor: Position) -> Option<(Range, String)> {
    let bytes = line_prefix.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    // Trailing alphabetic chars are the in-progress suffix; the run before
    // them must end in a digit for this to look like a numeric literal.
    let mut alpha = 0usize;
    while alpha < bytes.len() && bytes[bytes.len() - 1 - alpha].is_ascii_alphabetic() {
        alpha += 1;
    }
    let digits_end = bytes.len().saturating_sub(alpha);
    if digits_end == 0 || !bytes[digits_end - 1].is_ascii_digit() {
        return None;
    }
    // Walk back through the digit run (allowing `_` separators).
    let mut digits_start = digits_end;
    while digits_start > 0 {
        let c = bytes[digits_start - 1];
        if c.is_ascii_digit() || c == b'_' {
            digits_start -= 1;
        } else {
            break;
        }
    }
    let digits = line_prefix[digits_start..digits_end].to_string();
    // ASCII-only assumption keeps the column math simple — primate
    // identifiers and suffixes are ASCII.
    let span = (bytes.len() - digits_start) as u32;
    let start = Position {
        line: cursor.line,
        character: cursor.character.saturating_sub(span),
    };
    Some((Range { start, end: cursor }, digits))
}

/// Push unit-suffix completions appropriate for `declared` type. Duration
/// types get time suffixes, integer types get byte-size suffixes. Each
/// completion uses an explicit textEdit covering both the digits and any
/// in-progress alphabetic suffix, so the inserted text is the full
/// `<digits><suffix>` literal — and `filter_text` matches what the user
/// has typed so far so the items aren't filtered out by a leading-digit
/// prefix.
fn push_unit_suffix_completions(
    declared: &str,
    range: Range,
    digits: &str,
    completions: &mut Vec<CompletionItem>,
) {
    // Strip optional sugar / wrapper layers so we look at the underlying
    // primitive. We don't recurse through user aliases here — the worst
    // case is we miss offering suffixes for an alias of `duration`, which
    // is acceptable.
    let inner = declared
        .trim()
        .trim_end_matches('?')
        .strip_prefix("optional<")
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(declared)
        .trim();

    let suffixes: &[(&str, &str)] = match inner {
        "duration" => &[
            ("ns", "nanoseconds"),
            ("us", "microseconds"),
            ("ms", "milliseconds"),
            ("s", "seconds"),
            ("min", "minutes"),
            ("h", "hours"),
            ("d", "days"),
            ("w", "weeks (7d)"),
        ],
        "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" => &[
            ("B", "bytes"),
            ("KB", "kilobytes (1_000)"),
            ("MB", "megabytes (1_000_000)"),
            ("GB", "gigabytes"),
            ("TB", "terabytes"),
            ("KiB", "kibibytes (1_024)"),
            ("MiB", "mebibytes"),
            ("GiB", "gibibytes"),
            ("TiB", "tebibytes"),
        ],
        "f32" | "f64" => &[("%", "percentage (value / 100)")],
        _ => return,
    };

    for (suffix, detail) in suffixes {
        let inserted = format!("{}{}", digits, suffix);
        completions.push(CompletionItem {
            // The label itself is the full literal so editors that filter
            // completion items by prefix (matching the user's typed text
            // against the start of the label) include these in the menu.
            // Picking the item replaces the in-progress literal with the
            // shown text.
            label: inserted.clone(),
            kind: Some(CompletionItemKind::UNIT),
            detail: Some(detail.to_string()),
            // Sort by the suffix only so they don't all collide on the
            // leading digits when ordered alphabetically.
            sort_text: Some(format!("0_{}", suffix)),
            filter_text: Some(inserted.clone()),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: inserted,
            })),
            ..Default::default()
        });
    }
}

/// Look at the line prefix and decide what kind of completion is appropriate.
///
/// The detection is intentionally lenient: we want to offer type completions
/// even when the buffer doesn't parse cleanly (e.g. the user is mid-typing
/// `tuple<tup` inside a generic).
fn detect_completion_context(line_prefix: &str) -> CompletionContext {
    // Strip a trailing line comment, if any. Strings can't appear in type
    // expressions so we don't bother handling string literals containing `//`.
    let line = strip_line_comment(line_prefix);

    // Trim trailing partial identifier so we look at the structure *before* it.
    let before_partial = strip_trailing_word(line);
    let trimmed = before_partial.trim_start();

    // Empty or only-whitespace + partial → start of decl.
    if trimmed.is_empty() {
        return CompletionContext::DeclStart;
    }

    // Track generic depth (`<`/`>`) and whether we've seen a top-level `=`.
    // Top-level here means: not nested inside `<>`, `[]`, `{}`, `()`.
    let mut generic_depth: i32 = 0;
    let mut paren_depth: i32 = 0;
    let mut top_level_eq = false;
    let mut starts_with_kw_type = false;

    // Pre-check: does the trimmed line start with the `type ` keyword?
    if trimmed.starts_with("type ") || trimmed.starts_with("type\t") {
        starts_with_kw_type = true;
    }

    let chars = trimmed.chars().peekable();
    for c in chars {
        match c {
            '<' => generic_depth += 1,
            '>' => generic_depth = (generic_depth - 1).max(0),
            '[' | '{' | '(' => paren_depth += 1,
            ']' | '}' | ')' => paren_depth = (paren_depth - 1).max(0),
            '=' if generic_depth == 0 && paren_depth == 0 => top_level_eq = true,
            _ => {}
        }
    }

    // Inside generic brackets → always a type position.
    if generic_depth > 0 {
        return CompletionContext::TypePosition;
    }

    // After `=` in a `type X = ` decl → type position. After `=` in any other
    // decl → value position.
    if top_level_eq {
        if starts_with_kw_type {
            return CompletionContext::TypePosition;
        }
        return CompletionContext::ValuePosition;
    }

    // No `<` and no `=` yet — probably typing the leading type/keyword.
    CompletionContext::DeclStart
}

fn strip_line_comment(s: &str) -> &str {
    if let Some(idx) = s.find("//") {
        &s[..idx]
    } else {
        s
    }
}

fn strip_trailing_word(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    while end > 0 {
        let b = bytes[end - 1];
        if b.is_ascii_alphanumeric() || b == b'_' {
            end -= 1;
        } else {
            break;
        }
    }
    &s[..end]
}

fn push_decl_start_keywords(completions: &mut Vec<CompletionItem>) {
    for kw in &["namespace", "enum", "type"] {
        completions.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            sort_text: Some(format!("0_{}", kw)),
            ..Default::default()
        });
    }
}

fn push_primitive_types(completions: &mut Vec<CompletionItem>) {
    for kw in &[
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64", "bool", "string",
        "duration", "regex", "url",
    ] {
        completions.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("primitive".to_string()),
            sort_text: Some(format!("1_{}", kw)),
            ..Default::default()
        });
    }
}

fn push_container_constructors(completions: &mut Vec<CompletionItem>) {
    for (label, snippet) in &[
        ("array", "array<$1>"),
        ("optional", "optional<$1>"),
        ("map", "map<$1, $2>"),
        ("tuple", "tuple<$1>"),
    ] {
        completions.push(CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("type constructor".to_string()),
            insert_text: Some(snippet.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some(format!("2_{}", label)),
            ..Default::default()
        });
    }
}

fn push_local_user_types(
    completions: &mut Vec<CompletionItem>,
    enums: &[(String, String)],
    aliases: &[(String, String)],
) {
    for (name, detail) in enums {
        completions.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::ENUM),
            detail: Some(detail.clone()),
            sort_text: Some(format!("3_{}", name)),
            ..Default::default()
        });
    }
    for (name, detail) in aliases {
        completions.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some(detail.clone()),
            sort_text: Some(format!("3_{}", name)),
            ..Default::default()
        });
    }
}

fn push_workspace_user_types(
    completions: &mut Vec<CompletionItem>,
    enums: &[(String, String, String)], // (name, namespace, kind)
    aliases: &[(String, String, String)],
) {
    for (name, namespace, _) in enums {
        let qualified = if namespace.is_empty() {
            name.clone()
        } else {
            format!("{}::{}", namespace, name)
        };
        completions.push(CompletionItem {
            label: name.clone(),
            label_details: Some(CompletionItemLabelDetails {
                description: Some(namespace.clone()),
                detail: None,
            }),
            kind: Some(CompletionItemKind::ENUM),
            detail: Some(format!("enum in {}", namespace)),
            insert_text: Some(qualified),
            sort_text: Some(format!("4_{}", name)),
            ..Default::default()
        });
    }
    for (name, namespace, _) in aliases {
        let qualified = if namespace.is_empty() {
            name.clone()
        } else {
            format!("{}::{}", namespace, name)
        };
        completions.push(CompletionItem {
            label: name.clone(),
            label_details: Some(CompletionItemLabelDetails {
                description: Some(namespace.clone()),
                detail: None,
            }),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some(format!("alias in {}", namespace)),
            insert_text: Some(qualified),
            sort_text: Some(format!("4_{}", name)),
            ..Default::default()
        });
    }
}

// ---------- Hover & symbol-lookup helpers ----------

/// Extract the qualified path (`foo::bar::Baz`) at `position`. Allows the
/// cursor to be on any segment, including the `::` between segments.
fn qualified_path_at(content: &str, position: Position) -> Option<String> {
    // Find the requested line.
    let mut line_idx: u32 = 0;
    let mut line_start: usize = 0;
    for (i, b) in content.bytes().enumerate() {
        if line_idx == position.line {
            break;
        }
        if b == b'\n' {
            line_idx += 1;
            line_start = i + 1;
        }
    }
    if line_idx < position.line {
        return None;
    }
    let line_end = content[line_start..]
        .find('\n')
        .map(|n| line_start + n)
        .unwrap_or(content.len());
    let line = &content[line_start..line_end];

    // Convert character index (treated as char count, an approximation of
    // LSP's UTF-16 code units that matches for ASCII) to a byte index.
    let cursor_char = position.character as usize;
    let byte_offset = line
        .char_indices()
        .nth(cursor_char)
        .map(|(i, _)| i)
        .unwrap_or(line.len());

    let bytes = line.as_bytes();

    // Walk left to find path start.
    let mut start = byte_offset;
    while start > 0 {
        let b = bytes[start - 1];
        if is_path_char(b) {
            start -= 1;
        } else {
            break;
        }
    }
    // Walk right to find path end.
    let mut end = byte_offset;
    while end < bytes.len() && is_path_char(bytes[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }

    let raw = &line[start..end];
    // Trim leading/trailing `:` (could happen if cursor is right next to `::`).
    let trimmed = raw.trim_matches(':');
    if trimmed.is_empty() {
        return None;
    }
    // Reject paths that contain `:::` or other malformed sequences.
    if trimmed.contains(":::") {
        return None;
    }
    Some(trimmed.to_string())
}

fn is_path_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b':'
}

fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn source_location_to_lsp(loc: &crate::ir::SourceLocation) -> Option<Location> {
    let path = Path::new(&loc.file);
    let uri = path_to_uri(path)?;
    let line = loc.line.saturating_sub(1);
    let column = loc.column.saturating_sub(1);
    let length = loc.length.unwrap_or(0);
    Some(Location {
        uri,
        range: Range {
            start: Position {
                line,
                character: column,
            },
            end: Position {
                line,
                character: column + length,
            },
        },
    })
}

fn path_to_uri(path: &Path) -> Option<Uri> {
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let url = url::Url::from_file_path(&canon).ok()?;
    Uri::from_str(url.as_str()).ok()
}

fn make_hover(value: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    }
}

fn primitive_hover(name: &str) -> Option<String> {
    let desc = match name {
        "i8" | "i16" | "i32" | "i64" => "signed integer",
        "u8" | "u16" | "u32" | "u64" => "unsigned integer",
        "f32" | "f64" => "floating-point number",
        "bool" => "boolean",
        "string" => "UTF-8 string",
        "duration" => "duration (suffixed: 30s, 5min, 1h, ...)",
        "regex" => "regular expression",
        "url" => "URL",
        _ => return None,
    };
    Some(format!("```primate\n{}\n```\n\n{}", name, desc))
}

fn container_hover(name: &str) -> Option<String> {
    let (sig, desc) = match name {
        "array" => ("array<T>", "homogeneous array of `T`"),
        "optional" => ("optional<T>", "either a `T` or `none`"),
        "map" => ("map<K, V>", "key/value map"),
        "tuple" => ("tuple<A, B, ...>", "fixed-arity heterogeneous tuple"),
        _ => return None,
    };
    Some(format!("```primate\n{}\n```\n\n{}", sig, desc))
}

/// Pretty-print an IR `Type` as primate-source-like syntax (best effort).
fn format_type(t: &crate::types::Type) -> String {
    use crate::types::Type;
    match t {
        Type::I32 => "i32".into(),
        Type::I64 => "i64".into(),
        Type::U32 => "u32".into(),
        Type::U64 => "u64".into(),
        Type::F32 => "f32".into(),
        Type::F64 => "f64".into(),
        Type::Bool => "bool".into(),
        Type::String => "string".into(),
        Type::Duration => "duration".into(),
        Type::Regex => "regex".into(),
        Type::Url => "url".into(),
        Type::Array { element } => format!("array<{}>", format_type(element)),
        Type::FixedArray { element, length } => {
            format!("array<{}, {}>", format_type(element), length)
        }
        Type::Optional { inner } => format!("optional<{}>", format_type(inner)),
        Type::Map { key, value } => {
            format!("map<{}, {}>", format_type(key), format_type(value))
        }
        Type::Tuple { elements } => {
            let parts: Vec<String> = elements.iter().map(format_type).collect();
            format!("tuple<{}>", parts.join(", "))
        }
        Type::Enum { name, .. } => name.clone(),
        Type::Alias { name, .. } => name.clone(),
        Type::Struct { .. } => "<struct>".into(),
    }
}

fn cast_request<R>(req: Request) -> Result<(RequestId, R::Params), Request>
where
    R: lsp_types::request::Request,
    R::Params: serde::de::DeserializeOwned,
{
    let req_clone = req.clone();
    match req.extract(R::METHOD) {
        Ok(res) => Ok(res),
        Err(lsp_server::ExtractError::MethodMismatch(_)) => Err(req_clone),
        Err(lsp_server::ExtractError::JsonError { method, error }) => {
            eprintln!("JSON error for {}: {}", method, error);
            Err(req_clone)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_type_for_u32_const() {
        // The user is typing `u32 MAX_RETRIES = 3<cursor>`.
        let prefix = "u32 MAX_RETRIES = 3";
        assert_eq!(
            extract_declared_type_from_prefix(prefix),
            Some("u32".to_string())
        );
    }

    #[test]
    fn extracts_type_for_duration_const() {
        let prefix = "duration TIMEOUT = 30";
        assert_eq!(
            extract_declared_type_from_prefix(prefix),
            Some("duration".to_string())
        );
        // After typing a partial suffix.
        let prefix = "duration TIMEOUT = 30m";
        assert_eq!(
            extract_declared_type_from_prefix(prefix),
            Some("duration".to_string())
        );
    }

    #[test]
    fn pending_suffix_state_after_digit() {
        let prefix = "u32 X = 3";
        let pos = Position { line: 0, character: 9 };
        let (range, digits) = pending_suffix_state(prefix, pos).expect("digit yields state");
        // Range covers the digit run `3`; digits string is `"3"`.
        assert_eq!(range.start.character, 8);
        assert_eq!(range.end.character, 9);
        assert_eq!(digits, "3");
    }

    #[test]
    fn pending_suffix_state_with_partial_suffix() {
        let prefix = "duration X = 30m";
        let pos = Position { line: 0, character: 16 };
        let (range, digits) = pending_suffix_state(prefix, pos).expect("digits+alpha yields state");
        // Range covers `30m`; digits string is `"30"`.
        assert_eq!(range.start.character, 13);
        assert_eq!(range.end.character, 16);
        assert_eq!(digits, "30");
    }

    #[test]
    fn pending_suffix_state_no_digit() {
        let prefix = "LogLevel X = Inf";
        let pos = Position { line: 0, character: 16 };
        assert!(pending_suffix_state(prefix, pos).is_none());
    }

    #[test]
    fn unit_suffix_completions_for_u32() {
        let mut completions = Vec::new();
        let range = Range {
            start: Position { line: 0, character: 8 },
            end: Position { line: 0, character: 9 },
        };
        push_unit_suffix_completions("u32", range, "3", &mut completions);
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"3KB"), "got {:?}", labels);
        assert!(labels.contains(&"3MiB"), "got {:?}", labels);
        // Filter text and label match so the editor's prefix filter accepts them.
        let kb = completions.iter().find(|c| c.label == "3KB").unwrap();
        assert_eq!(kb.filter_text.as_deref(), Some("3KB"));
    }

    #[test]
    fn unit_suffix_completions_for_duration() {
        let mut completions = Vec::new();
        let range = Range {
            start: Position { line: 0, character: 13 },
            end: Position { line: 0, character: 17 },
        };
        push_unit_suffix_completions("duration", range, "30", &mut completions);
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"30ms"), "got {:?}", labels);
        assert!(labels.contains(&"30min"), "got {:?}", labels);
        let min = completions.iter().find(|c| c.label == "30min").unwrap();
        assert_eq!(min.filter_text.as_deref(), Some("30min"));
    }

    #[test]
    fn ctx_empty_line_is_decl_start() {
        assert_eq!(
            detect_completion_context(""),
            CompletionContext::DeclStart
        );
        assert_eq!(
            detect_completion_context("    "),
            CompletionContext::DeclStart
        );
    }

    #[test]
    fn ctx_partial_keyword_at_start_is_decl_start() {
        assert_eq!(
            detect_completion_context("tup"),
            CompletionContext::DeclStart
        );
        assert_eq!(
            detect_completion_context("  ar"),
            CompletionContext::DeclStart
        );
    }

    #[test]
    fn ctx_inside_generic_is_type_position() {
        // The user's reported case: `tuple<tup|>` with cursor between `tup` and `>`.
        // Line prefix at the cursor is `tuple<tup`.
        assert_eq!(
            detect_completion_context("tuple<tup"),
            CompletionContext::TypePosition
        );
        assert_eq!(
            detect_completion_context("tuple<"),
            CompletionContext::TypePosition
        );
        assert_eq!(
            detect_completion_context("array<"),
            CompletionContext::TypePosition
        );
        assert_eq!(
            detect_completion_context("map<string, "),
            CompletionContext::TypePosition
        );
        assert_eq!(
            detect_completion_context("optional<array<"),
            CompletionContext::TypePosition
        );
    }

    #[test]
    fn ctx_after_eq_in_type_alias_is_type_position() {
        assert_eq!(
            detect_completion_context("type Port = "),
            CompletionContext::TypePosition
        );
        assert_eq!(
            detect_completion_context("type Port = u"),
            CompletionContext::TypePosition
        );
    }

    #[test]
    fn ctx_after_eq_in_const_decl_is_value_position() {
        assert_eq!(
            detect_completion_context("u32 PORT = "),
            CompletionContext::ValuePosition
        );
        assert_eq!(
            detect_completion_context("Status STATE = "),
            CompletionContext::ValuePosition
        );
    }

    #[test]
    fn ctx_closed_generic_returns_to_decl_start() {
        // After `tuple<i32>` the depth is 0 again — back to a decl-start-ish
        // position (we're between the type and the name).
        assert_eq!(
            detect_completion_context("tuple<i32> "),
            CompletionContext::DeclStart
        );
    }

    #[test]
    fn ctx_ignores_line_comments() {
        // `<` inside a `//` comment shouldn't open a generic.
        assert_eq!(
            detect_completion_context("// foo<bar "),
            CompletionContext::DeclStart
        );
    }

    #[test]
    fn line_prefix_extracts_current_line() {
        let src = "namespace foo\nu32 X = 1\ntuple<tup";
        let pos = Position {
            line: 2,
            character: 9,
        };
        assert_eq!(line_prefix_at(src, pos), "tuple<tup");
    }

    #[test]
    fn qualified_path_extracts_simple_name() {
        let src = "u32 PORT = 1\n";
        // cursor on `PORT`, char 6.
        let pos = Position {
            line: 0,
            character: 6,
        };
        assert_eq!(
            qualified_path_at(src, pos),
            Some("PORT".to_string())
        );
    }

    #[test]
    fn qualified_path_extracts_qualified_name() {
        let src = "examples::constants::limits::Port FOO = 1\n";
        // cursor on `Port` (e.g. char 30).
        let pos = Position {
            line: 0,
            character: 30,
        };
        assert_eq!(
            qualified_path_at(src, pos),
            Some("examples::constants::limits::Port".to_string())
        );
    }

    #[test]
    fn qualified_path_handles_cursor_on_namespace_segment() {
        let src = "examples::constants::limits::Port FOO = 1\n";
        // cursor on `constants` (char 12).
        let pos = Position {
            line: 0,
            character: 12,
        };
        assert_eq!(
            qualified_path_at(src, pos),
            Some("examples::constants::limits::Port".to_string())
        );
    }

    #[test]
    fn qualified_path_returns_none_on_punctuation() {
        let src = "u32 PORT = 1\n";
        let pos = Position {
            line: 0,
            character: 9, // on `=`
        };
        assert_eq!(qualified_path_at(src, pos), None);
    }
}
