import {
  workspace,
  ExtensionContext,
  window,
  commands,
  languages,
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

let client: LanguageClient;

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

export function activate(context: ExtensionContext) {
  const config = workspace.getConfiguration("primate");
  const serverPath = config.get<string>("server.path") || "primate";

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
    // Activate for the `primate` language (registered in package.json's
    // `contributes.languages`). The synchronize block also forwards
    // primate.toml changes so the LSP can re-resolve namespaces when
    // the project's input directory changes.
    documentSelector: [
      { scheme: "file", language: "primate" },
      { scheme: "file", pattern: "**/primate.toml" },
    ],
    synchronize: {
      fileEvents: [
        workspace.createFileSystemWatcher("**/*.prim"),
        workspace.createFileSystemWatcher("**/primate.toml"),
      ],
    },
    outputChannelName: "primate LSP",
  };

  client = new LanguageClient(
    "primate",
    "primate LSP",
    serverOptions,
    clientOptions,
  );

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

  client
    .start()
    .catch((e) =>
      window.showErrorMessage(`primate LSP failed to start: ${e}`),
    );

  context.subscriptions.push(
    commands.registerCommand("primate.restartServer", async () => {
      try {
        await client.stop();
      } catch {
        // ignore — we're about to restart it
      }
      client
        .start()
        .catch((e) =>
          window.showErrorMessage(`primate LSP failed to restart: ${e}`),
        );
      window.showInformationMessage("primate LSP server restarted");
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
