import {
  workspace,
  ExtensionContext,
  window,
  commands,
  languages,
  env,
  OutputChannel,
  Uri,
  Position,
  Location,
  Range,
  DefinitionProvider,
  ReferenceProvider,
  TextDocument,
  CancellationToken,
  Definition,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  Executable,
  RequestType,
  TextDocumentIdentifier,
  Position as LSPPosition,
  Location as LSPLocation,
} from "vscode-languageclient/node";

import { spawnSync } from "child_process";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

let client: LanguageClient | undefined;
let logChannel: OutputChannel | undefined;

function getLog(): OutputChannel {
  if (!logChannel) logChannel = window.createOutputChannel("primate LSP");
  return logChannel;
}

const INSTALL_DOCS = "https://valtyr.github.io/primate/editors/vscode.html";

// Custom requests primate's LSP exposes for source-mapped navigation between
// .prim files and the generated targets (e.g. jump from `LogLevel.Info` in a
// generated .ts file back to its source declaration in limits.prim).
interface GeneratedPositionsParams {
  text_document: TextDocumentIdentifier;
  position: LSPPosition;
}
const GeneratedPositionsRequest = new RequestType<
  GeneratedPositionsParams,
  LSPLocation[],
  void
>("primate/generatedPositions");

interface ResolveSourceLocationParams {
  uri: string;
  line: number;
}
const ResolveSourceLocationRequest = new RequestType<
  ResolveSourceLocationParams,
  LSPLocation | null,
  void
>("primate/resolveSourceLocation");

// Languages we walk when chasing a generated symbol back to its `.prim` source.
const GENERATED_LANGUAGES = ["typescript", "javascript", "rust", "python"];

/**
 * Locate the `primate` executable.
 *
 * VS Code is often launched from a dock or launcher with a minimal PATH, so
 * `primate` installed by cargo, mise, asdf, Homebrew, etc. is frequently not
 * visible to the extension host even though it works in the user's terminal.
 * We therefore try, in order:
 *
 *   1. `primate.server.path` if the user set it (supports `~` and
 *      `${workspaceFolder}`).
 *   2. The extension host's own PATH.
 *   3. The user's login+interactive shell PATH (`$SHELL -ilc 'command -v …'`),
 *      run from the workspace folder so per-project tool managers such as
 *      mise/asdf/direnv resolve the project's version.
 *   4. `mise which primate`, in case mise is installed but not activated in
 *      the shell rc files.
 *   5. Well-known install directories (cargo, mise/asdf/proto shims,
 *      Homebrew, ~/.local/bin, nix).
 */
export function resolvePrimateBinary(): string | undefined {
  const config = workspace.getConfiguration("primate");
  const configured = (config.get<string>("server.path") || "").trim();
  const workspaceDir = workspace.workspaceFolders?.[0]?.uri.fsPath;
  const home = os.homedir();
  const exe = process.platform === "win32" ? "primate.exe" : "primate";

  const expand = (p: string): string => {
    let out = p;
    if (out.startsWith("~")) out = path.join(home, out.slice(1));
    if (workspaceDir) out = out.replace(/\$\{workspaceFolder\}/g, workspaceDir);
    return out;
  };

  const isExecutable = (p: string): boolean => {
    try {
      fs.accessSync(p, fs.constants.X_OK);
      return fs.statSync(p).isFile();
    } catch {
      return false;
    }
  };

  const findOnPath = (name: string, pathVar: string | undefined): string | undefined => {
    if (!pathVar) return undefined;
    const exts =
      process.platform === "win32"
        ? (process.env.PATHEXT || ".EXE;.CMD;.BAT").split(";").concat([""])
        : [""];
    for (const dir of pathVar.split(path.delimiter)) {
      if (!dir) continue;
      for (const ext of exts) {
        const candidate = path.join(dir, name + ext);
        if (isExecutable(candidate)) return candidate;
      }
    }
    return undefined;
  };

  const log = getLog();
  const run = (
    cmd: string,
    args: string[],
    label: string,
  ): string | undefined => {
    try {
      const r = spawnSync(cmd, args, {
        cwd: workspaceDir,
        timeout: 5000,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
      });
      const out = (r.stdout || "").trim().split(/\r?\n/).pop() || "";
      if (r.status === 0 && out && isExecutable(out)) return out;
      log.appendLine(
        `  ${label}: no result (exit ${r.status}${r.error ? ", " + r.error.message : ""})`,
      );
    } catch (e) {
      log.appendLine(`  ${label}: ${e}`);
    }
    return undefined;
  };

  log.appendLine("Resolving primate binary…");

  // 1. Explicit setting. A bare name is treated as something to search for;
  //    anything with a path separator is used as-is.
  let name = exe;
  if (configured && configured !== "primate") {
    const expanded = expand(configured);
    if (expanded.includes(path.sep) || expanded.includes("/")) {
      if (isExecutable(expanded)) {
        log.appendLine(`  using primate.server.path: ${expanded}`);
        return expanded;
      }
      log.appendLine(
        `  primate.server.path is set to ${expanded} but it is not an executable file`,
      );
      return undefined;
    }
    name = expanded;
  }

  // 2. Our own PATH.
  const onPath = findOnPath(name, process.env.PATH);
  if (onPath) {
    log.appendLine(`  found on PATH: ${onPath}`);
    return onPath;
  }

  // 3. The user's shell. mise/asdf/direnv activation normally lives in the
  //    interactive rc file, so ask a login + interactive shell.
  if (process.platform !== "win32") {
    const shell = process.env.SHELL || "/bin/sh";
    const found = run(
      shell,
      ["-ilc", `command -v '${name.replace(/'/g, "'\\''")}' 2>/dev/null`],
      `shell (${shell})`,
    );
    if (found) {
      log.appendLine(`  found via login shell: ${found}`);
      return found;
    }
  }

  // 4. mise directly, for installs where mise isn't activated in the shell.
  const miseCandidates = [
    findOnPath("mise", process.env.PATH),
    path.join(home, ".local", "bin", "mise"),
    "/opt/homebrew/bin/mise",
    "/usr/local/bin/mise",
    path.join(home, ".cargo", "bin", "mise"),
  ].filter((p): p is string => !!p && isExecutable(p));
  for (const mise of miseCandidates) {
    const found = run(mise, ["which", name], `mise (${mise})`);
    if (found) {
      log.appendLine(`  found via mise: ${found}`);
      return found;
    }
    break; // one working mise is enough to ask
  }

  // 5. Well-known install locations and tool-manager shim directories.
  const xdgData = process.env.XDG_DATA_HOME || path.join(home, ".local", "share");
  const miseData = process.env.MISE_DATA_DIR || path.join(xdgData, "mise");
  const asdfDir = process.env.ASDF_DATA_DIR || path.join(home, ".asdf");
  const dirs = [
    path.join(home, ".cargo", "bin"),
    path.join(miseData, "shims"),
    path.join(asdfDir, "shims"),
    path.join(process.env.PROTO_HOME || path.join(home, ".proto"), "shims"),
    path.join(home, ".local", "bin"),
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/home/linuxbrew/.linuxbrew/bin",
    path.join(home, ".nix-profile", "bin"),
    "/run/current-system/sw/bin",
  ];
  for (const dir of dirs) {
    const candidate = path.join(dir, name);
    if (isExecutable(candidate)) {
      log.appendLine(`  found in ${dir}: ${candidate}`);
      return candidate;
    }
  }

  log.appendLine("  primate not found");
  return undefined;
}

function showNotFound(): void {
  const settings = "Open Settings";
  const docs = "Install Instructions";
  const output = "Show Log";
  window
    .showErrorMessage(
      "primate: could not find the `primate` executable. Install it " +
        "(e.g. `cargo install primate --locked`), or set `primate.server.path`.",
      settings,
      docs,
      output,
    )
    .then((choice) => {
      if (choice === settings) {
        commands.executeCommand("workbench.action.openSettings", "primate.server.path");
      } else if (choice === docs) {
        env.openExternal(Uri.parse(INSTALL_DOCS));
      } else if (choice === output) {
        getLog().show();
      }
    });
}

function buildClient(serverPath: string): LanguageClient {
  const run: Executable = {
    command: serverPath,
    args: ["lsp"],
    transport: TransportKind.stdio,
  };

  const serverOptions: ServerOptions = {
    run,
    debug: run,
  };

  const clientOptions: LanguageClientOptions = {
    // Only activate for primate-language documents (registered in
    // package.json's `contributes.languages`). primate.toml is a
    // regular TOML file served by whichever TOML LSP the user has —
    // we previously selected it here too, which caused VS Code to
    // ask `primate lsp` for completions inside primate.toml and
    // get back primate-DSL suggestions. Watching the file via
    // `synchronize.fileEvents` is enough to let the server notice
    // input-directory changes; it doesn't need to handle requests
    // for the file.
    documentSelector: [{ scheme: "file", language: "primate" }],
    synchronize: {
      fileEvents: [
        workspace.createFileSystemWatcher("**/*.prim"),
        workspace.createFileSystemWatcher("**/primate.toml"),
      ],
    },
    outputChannel: getLog(),
  };

  return new LanguageClient("primate", "primate LSP", serverOptions, clientOptions);
}

/** Resolve the binary and (re)start the client. Returns false if not found. */
async function startClient(): Promise<boolean> {
  const serverPath = resolvePrimateBinary();
  if (!serverPath) {
    showNotFound();
    return false;
  }
  client = buildClient(serverPath);
  try {
    await client.start();
  } catch (e) {
    window.showErrorMessage(`primate LSP failed to start (${serverPath}): ${e}`);
    return false;
  }
  return true;
}

export function activate(context: ExtensionContext) {
  context.subscriptions.push(getLog());

  // Cross-target navigation. `provideReferences` jumps from a constant in a
  // .prim file to its generated-target callsites; `provideDefinition` does the
  // reverse — given a symbol in generated code, it returns the `.prim` source.
  context.subscriptions.push(
    languages.registerReferenceProvider(
      { scheme: "file", language: "primate" },
      new PrimateReferenceProvider(),
    ),
    languages.registerDefinitionProvider(
      GENERATED_LANGUAGES,
      new PrimateDefinitionProvider(),
    ),
  );

  void startClient();

  // Re-resolve the binary on restart so a fresh install or a changed
  // `primate.server.path` is picked up without reloading the window.
  context.subscriptions.push(
    commands.registerCommand("primate.restartServer", async () => {
      try {
        await client?.stop();
      } catch {
        // ignore — we're about to restart it
      }
      client = undefined;
      if (await startClient()) {
        window.showInformationMessage("primate LSP server restarted");
      }
    }),
    workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("primate.server.path")) {
        commands.executeCommand("primate.restartServer");
      }
    }),
  );
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}

/**
 * From a constant in a `.prim` file, find every callsite in generated code.
 *
 * Asks the LSP for the symbol's positions in generated files (via the
 * sourcemap), then runs VS Code's built-in reference provider against each
 * of those positions to collect callsites.
 */
class PrimateReferenceProvider implements ReferenceProvider {
  async provideReferences(
    document: TextDocument,
    position: Position,
    _context: { includeDeclaration: boolean },
    token: CancellationToken,
  ): Promise<Location[]> {
    if (!client) return [];

    const params: GeneratedPositionsParams = {
      text_document: { uri: document.uri.toString() },
      position: { line: position.line, character: position.character },
    };

    const generatedLocations = await client.sendRequest(
      GeneratedPositionsRequest,
      params,
      token,
    );
    if (!generatedLocations || generatedLocations.length === 0) return [];

    const results: Location[] = [];
    for (const loc of generatedLocations) {
      const refs = await commands.executeCommand<Location[]>(
        "vscode.executeReferenceProvider",
        Uri.parse(loc.uri),
        new Position(loc.range.start.line, loc.range.start.character),
      );
      if (refs) results.push(...refs);
    }
    return results;
  }
}

/**
 * From a generated symbol, jump back to its `.prim` source.
 *
 * Runs VS Code's built-in definition provider for the symbol, then asks the
 * LSP whether any of the resulting locations are inside generated files —
 * if so, it resolves them back to the originating `.prim` line via the
 * sourcemap.
 */
class PrimateDefinitionProvider implements DefinitionProvider {
  // Re-entrancy guard: we call back into the built-in definition provider,
  // which can re-enter this method if a chain of definitions exists.
  private processing = new Set<string>();

  async provideDefinition(
    document: TextDocument,
    position: Position,
    token: CancellationToken,
  ): Promise<Definition | null> {
    if (!client) return null;

    const key = `${document.uri.toString()}:${position.line}:${position.character}`;
    if (this.processing.has(key)) return null;

    try {
      this.processing.add(key);

      const definitions = await commands.executeCommand<
        (Location | { targetUri: Uri; targetRange: Range })[]
      >("vscode.executeDefinitionProvider", document.uri, position);

      if (!definitions || definitions.length === 0) return null;

      for (const def of definitions) {
        let uri: Uri;
        let line: number;
        if ("targetUri" in def) {
          uri = def.targetUri;
          line = def.targetRange.start.line;
        } else {
          uri = def.uri;
          line = def.range.start.line;
        }

        const sourceLoc = await client.sendRequest(
          ResolveSourceLocationRequest,
          { uri: uri.toString(), line },
          token,
        );
        if (sourceLoc) {
          return new Location(
            Uri.parse(sourceLoc.uri),
            new Range(
              sourceLoc.range.start.line,
              sourceLoc.range.start.character,
              sourceLoc.range.end.line,
              sourceLoc.range.end.character,
            ),
          );
        }
      }
      return null;
    } finally {
      this.processing.delete(key);
    }
  }
}
