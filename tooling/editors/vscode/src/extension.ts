import * as path from 'path';
import { workspace, ExtensionContext } from 'vscode';

import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind
} from 'vscode-languageclient/node';

let client: LanguageClient;

export function activate(context: ExtensionContext) {
  // Assuming `rad lsp` is in the PATH or we can find it
  let serverExecutable = 'rad';
  let serverArgs = ['lsp'];

  let serverOptions: ServerOptions = {
    run: { command: serverExecutable, args: serverArgs, transport: TransportKind.stdio },
    debug: {
      command: serverExecutable,
      args: serverArgs,
      transport: TransportKind.stdio,
    }
  };

  let clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'rad' }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher('**/*.rad')
    }
  };

  client = new LanguageClient(
    'radLanguageServer',
    'Rad Language Server',
    serverOptions,
    clientOptions
  );

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}