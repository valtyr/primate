import * as path from "path";
import { workspace, ExtensionContext, window, commands, languages, Uri, Position, Location, Range, DefinitionProvider, ReferenceProvider, TextDocument, CancellationToken, ProviderResult, Definition } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  Executable,
  RequestType,
  TextDocumentIdentifier,
  Position as LSPPosition,
  Location as LSPLocation
} from "vscode-languageclient/node";

let client: LanguageClient;

// Custom LSP Request Types
interface GeneratedPositionsParams {
  text_document: TextDocumentIdentifier;
  position: LSPPosition;
}

const GeneratedPositionsRequest = new RequestType<GeneratedPositionsParams, LSPLocation[], void>("primate/generatedPositions");

interface ResolveSourceLocationParams {
  uri: string;
  line: number;
}

const ResolveSourceLocationRequest = new RequestType<ResolveSourceLocationParams, LSPLocation | null, void>("primate/resolveSourceLocation");

export function activate(context: ExtensionContext) {
  try {
    console.log('Activating primate-vscode extension...');
    
    // Get the server path from configuration
    const config = workspace.getConfiguration("primate");
    const serverPath = config.get<string>("server.path") || "primate";
    console.log(`Using server path: ${serverPath}`);

    // Server options
    const run: Executable = {
      command: serverPath,
      args: ["lsp"],
      transport: TransportKind.stdio,
    };

    const serverOptions: ServerOptions = {
      run,
      debug: run,
    };

    // Client options
    const clientOptions: LanguageClientOptions = {
      documentSelector: [
        { scheme: "file", language: "toml", pattern: "**/*.c.toml" },
        { scheme: "file", language: "toml", pattern: "**/primate.toml" },
      ],
      synchronize: {
        fileEvents: [
          workspace.createFileSystemWatcher("**/*.c.toml"),
          workspace.createFileSystemWatcher("**/primate.toml"),
        ],
      },
      outputChannelName: "primate LSP",
    };

    // Create the client and start it
    client = new LanguageClient(
      "primate",
      "primate LSP",
      serverOptions,
      clientOptions,
    );

    // Register Providers
    context.subscriptions.push(
      languages.registerReferenceProvider(
        { scheme: "file", language: "toml", pattern: "**/*.c.toml" },
        new CConsttReferenceProvider()
      ),
      languages.registerDefinitionProvider(
        ["typescript", "javascript", "rust", "python"],
        new CConsttDefinitionProvider()
      )
    );

    // Start the client. This will also launch the server
    client.start().then(() => {
        console.log("primate LSP client started successfully.");
    }).catch(e => {
        console.error("primate LSP client failed to start:", e);
    });

    // Register restart command
    context.subscriptions.push(
      commands.registerCommand("primate.restartServer", async () => {
        if (client) {
          try {
             await client.stop();
          } catch (e) {
             console.error("Failed to stop client:", e);
          }
        }
        client.start().catch(e => console.error("Failed to restart client:", e));
        window.showInformationMessage("primate LSP server restarted");
      })
    );
    
    console.log('Congratulations, your extension "primate-vscode" is now active!');
  } catch (e) {
    console.error("Failed to activate primate-vscode:", e);
    window.showErrorMessage(`Failed to activate primate extension: ${e}`);
  }
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

/**
 * Provides references from .c.toml source to application code
 */
class CConsttReferenceProvider implements ReferenceProvider {
  async provideReferences(
    document: TextDocument,
    position: Position,
    context: { includeDeclaration: boolean },
    token: CancellationToken
  ): Promise<Location[]> {
    if (!client) return [];

    try {
      // 1. Ask LSP for generated positions
      const params: GeneratedPositionsParams = {
        text_document: { uri: document.uri.toString() },
        position: { line: position.line, character: position.character },
      };
      
      const generatedLocations = await client.sendRequest(GeneratedPositionsRequest, params, token);
      if (!generatedLocations || generatedLocations.length === 0) return [];

      const results: Location[] = [];

      // 2. For each generated location, ask VS Code for references
      for (const loc of generatedLocations) {
        const uri = Uri.parse(loc.uri);
        const pos = new Position(loc.range.start.line, loc.range.start.character);
        
        // Execute built-in reference provider
        const refs = await commands.executeCommand<Location[]>(
          "vscode.executeReferenceProvider",
          uri,
          pos
        );

        if (refs) {
          results.push(...refs);
        }
      }

      return results;
    } catch (e) {
      console.error("Error providing references:", e);
      return [];
    }
  }
}

/**
 * Provides definitions from application code back to .c.toml source
 */
class CConsttDefinitionProvider implements DefinitionProvider {
  // Re-entrancy guard to prevent infinite recursion
  private processing = new Set<string>();

  async provideDefinition(
    document: TextDocument,
    position: Position,
    token: CancellationToken
  ): Promise<Definition | null> {
    if (!client) return null;

    const key = `${document.uri.toString()}:${position.line}:${position.character}`;
    if (this.processing.has(key)) {
      return null;
    }

    try {
      this.processing.add(key);

      // 1. Ask standard providers where this symbol is defined
      const definitions = await commands.executeCommand<(Location | { targetUri: Uri, targetRange: Range })[]>(
        "vscode.executeDefinitionProvider",
        document.uri,
        position
      );

      if (!definitions || definitions.length === 0) return null;

      // 2. Check if any definition maps back to source
      for (const def of definitions) {
        // Handle both Location and LocationLink
        let uri: Uri;
        let line: number;

        if ("targetUri" in def) {
           uri = def.targetUri;
           line = def.targetRange.start.line;
        } else {
           uri = def.uri;
           line = def.range.start.line;
        }

        // Ask LSP to resolve
        const params: ResolveSourceLocationParams = {
          uri: uri.toString(),
          line: line,
        };

        const sourceLoc = await client.sendRequest(ResolveSourceLocationRequest, params, token);

        if (sourceLoc) {
          const sourceUri = Uri.parse(sourceLoc.uri);
          const sourceRange = new Range(
            sourceLoc.range.start.line,
            sourceLoc.range.start.character,
            sourceLoc.range.end.line,
            sourceLoc.range.end.character
          );
          return new Location(sourceUri, sourceRange);
        }
      }

      return null;
    } catch (e) {
      console.error("Error providing definition:", e);
      return null;
    } finally {
      this.processing.delete(key);
    }
  }
}
