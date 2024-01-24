/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from 'isl-server/src/logger';
import type {ClientToServerMessage, ServerToClientMessage} from 'isl/src/types';

import packageJson from '../package.json';
import {executeVSCodeCommand} from './commands';
import {getCLICommand} from './config';
import {locale, t} from './i18n';
import {VSCodePlatform} from './vscodePlatform';
import crypto from 'crypto';
import {onClientConnection} from 'isl-server/src';
import {deserializeFromString, serializeToString} from 'isl/src/serialize';
import {unwrap} from 'shared/utils';
import * as vscode from 'vscode';

let islPanelOrView: vscode.WebviewPanel | vscode.WebviewView | undefined = undefined;
let hasOpenedISLWebviewBeforeState = false;

const viewType = 'sapling.isl';

function createOrFocusISLWebview(
  context: vscode.ExtensionContext,
  logger: Logger,
): vscode.WebviewPanel | vscode.WebviewView {
  // Try to re-use existing ISL panel/view
  if (islPanelOrView) {
    isPanel(islPanelOrView) ? islPanelOrView.reveal() : islPanelOrView.show();
    return islPanelOrView;
  }
  // Otherwise, create a new panel/view

  const column = vscode.window.activeTextEditor?.viewColumn ?? vscode.ViewColumn.One;

  islPanelOrView = populateAndSetISLWebview(
    context,
    vscode.window.createWebviewPanel(viewType, t('isl.title'), column, getWebviewOptions(context)),
    logger,
  );
  return unwrap(islPanelOrView);
}

function getWebviewOptions(
  context: vscode.ExtensionContext,
): vscode.WebviewOptions & vscode.WebviewPanelOptions {
  return {
    enableScripts: true,
    retainContextWhenHidden: true,
    // Restrict the webview to only loading content from our extension's `webview` directory.
    localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, 'dist/webview')],
  };
}

function shouldUseWebviewView(): boolean {
  return vscode.workspace.getConfiguration('sapling.isl').get<boolean>('showInSidebar') ?? false;
}

export function hasOpenedISLWebviewBefore() {
  return hasOpenedISLWebviewBeforeState;
}

export function registerISLCommands(
  context: vscode.ExtensionContext,
  logger: Logger,
): vscode.Disposable {
  const webviewViewProvider = new ISLWebviewViewProvider(context, logger);
  return vscode.Disposable.from(
    vscode.commands.registerCommand('sapling.open-isl', () => {
      if (shouldUseWebviewView()) {
        // just open the sidebar view
        executeVSCodeCommand('sapling.isl.focus');
        return;
      }
      try {
        createOrFocusISLWebview(context, logger);
      } catch (err: unknown) {
        vscode.window.showErrorMessage(`error opening isl: ${err}`);
      }
    }),
    vscode.commands.registerCommand('sapling.close-isl', () => {
      if (!islPanelOrView) {
        return;
      }
      if (isPanel(islPanelOrView)) {
        islPanelOrView.dispose();
      } else {
        // close sidebar entirely
        executeVSCodeCommand('workbench.action.closeSidebar');
      }
    }),
    registerDeserializer(context, logger),
    vscode.window.registerWebviewViewProvider(viewType, webviewViewProvider, {
      webviewOptions: {
        retainContextWhenHidden: true,
      },
    }),
    vscode.workspace.onDidChangeConfiguration(e => {
      // if we start using ISL as a view, dispose the panel
      if (e.affectsConfiguration('sapling.isl.showInSidebar')) {
        if (islPanelOrView && isPanel(islPanelOrView) && shouldUseWebviewView()) {
          islPanelOrView.dispose();
        }
      }
    }),
  );
}

function registerDeserializer(context: vscode.ExtensionContext, logger: Logger) {
  // Make sure we register a serializer in activation event
  return vscode.window.registerWebviewPanelSerializer(viewType, {
    deserializeWebviewPanel(webviewPanel: vscode.WebviewPanel, _state: unknown) {
      if (shouldUseWebviewView()) {
        // if we try to deserialize a panel while we're trying to use view, destroy the panel and open the sidebar instead
        webviewPanel.dispose();
        executeVSCodeCommand('sapling.isl.focus');
        return Promise.resolve();
      }
      // Reset the webview options so we use latest uri for `localResourceRoots`.
      webviewPanel.webview.options = getWebviewOptions(context);
      populateAndSetISLWebview(context, webviewPanel, logger);
      return Promise.resolve();
    },
  });
}

/**
 * Provides the ISL webview contents as a VS Code Webview View, aka a webview that lives in the sidebar/bottom
 * rather than an editor pane. We always register this provider, even if the user doesn't have the config enabled
 * that shows this view.
 */
class ISLWebviewViewProvider implements vscode.WebviewViewProvider {
  constructor(private extensionContext: vscode.ExtensionContext, private logger: Logger) {}

  resolveWebviewView(webviewView: vscode.WebviewView): void | Thenable<void> {
    webviewView.webview.options = getWebviewOptions(this.extensionContext);
    populateAndSetISLWebview(this.extensionContext, webviewView, this.logger);
  }
}

function isPanel(
  panelOrView: vscode.WebviewPanel | vscode.WebviewView,
): panelOrView is vscode.WebviewPanel {
  // panels have a .reveal property, views have .show
  return (panelOrView as vscode.WebviewPanel).reveal !== undefined;
}

function populateAndSetISLWebview<W extends vscode.WebviewPanel | vscode.WebviewView>(
  context: vscode.ExtensionContext,
  panelOrView: W,
  logger: Logger,
): W {
  logger.info(`Populating ISL webview ${isPanel(panelOrView) ? 'panel' : 'view'}`);
  hasOpenedISLWebviewBeforeState = true;
  if (isPanel(panelOrView)) {
    islPanelOrView = panelOrView;
    panelOrView.iconPath = vscode.Uri.joinPath(
      context.extensionUri,
      'resources',
      'Sapling_favicon-light-green-transparent.svg',
    );
  }
  panelOrView.webview.html = htmlForISLWebview(
    context,
    panelOrView.webview,
    isPanel(panelOrView) ? 'panel' : 'view',
  );

  const disposeConnection = onClientConnection({
    postMessage(message: string) {
      return panelOrView.webview.postMessage(message) as Promise<boolean>;
    },
    onDidReceiveMessage(handler) {
      return panelOrView.webview.onDidReceiveMessage(m => {
        const isBinary = m instanceof ArrayBuffer;
        handler(m, isBinary);
      });
    },
    cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? process.cwd(), // TODO
    platform: VSCodePlatform,
    logger,
    command: getCLICommand(),
    version: packageJson.version,
  });

  panelOrView.onDidDispose(() => {
    if (isPanel(panelOrView)) {
      logger.info('Disposing ISL panel');
      islPanelOrView = undefined;
    } else {
      logger.info('Disposing ISL view');
    }
    disposeConnection();
  });

  return panelOrView;
}

export function fetchUIState(): Promise<{state: string} | undefined> {
  if (islPanelOrView == null) {
    return Promise.resolve(undefined);
  }

  return new Promise(resolve => {
    let dispose: vscode.Disposable | undefined = islPanelOrView?.webview.onDidReceiveMessage(
      (m: string) => {
        try {
          const data = deserializeFromString(m) as ClientToServerMessage;
          if (data.type === 'platform/gotUiState') {
            dispose?.dispose();
            dispose = undefined;
            resolve({state: data.state});
          }
        } catch {}
      },
    );

    islPanelOrView?.webview.postMessage(
      serializeToString({type: 'platform/getUiState'} as ServerToClientMessage),
    );
  });
}

function htmlForISLWebview(
  context: vscode.ExtensionContext,
  webview: vscode.Webview,
  kind: 'panel' | 'view',
) {
  // Only allow accessing resources relative to webview dir,
  // and make paths relative to here.
  const baseUri = webview.asWebviewUri(
    vscode.Uri.joinPath(context.extensionUri, 'dist', 'webview'),
  );

  const scriptUri = 'isl.js';
  const stylesMainUri = 'isl.css';

  // Use a nonce to only allow specific scripts to be run
  const nonce = getNonce();

  const loadingText = t('isl.loading-text');
  const titleText = t('isl.title');

  const extraRootClass = `webview-${kind}`;

  const CSP = [
    `default-src ${webview.cspSource}`,
    `style-src ${webview.cspSource}`,
    // vscode-webview-ui needs to use style-src-elem without the nonce
    `style-src-elem ${webview.cspSource} 'unsafe-inline'`,
    `font-src ${webview.cspSource} data:`,
    `img-src ${webview.cspSource} https: data:`,
    `script-src 'nonce-${nonce}' 'wasm-unsafe-eval'`,
    `script-src-elem 'nonce-${nonce}'`,
  ].join('; ');

  return `<!DOCTYPE html>
	<html lang="en">
	<head>
		<meta charset="UTF-8">
		<meta http-equiv="Content-Security-Policy" content="${CSP}">
		<meta name="viewport" content="width=device-width, initial-scale=1.0">
		<base href="${baseUri}/">
		<title>${titleText}</title>
		<link href="${stylesMainUri}" rel="stylesheet">
		<script nonce="${nonce}">
			window.saplingLanguage = "${locale /* important: locale has already been validated */}";
      window.webpackNonce = "${nonce}";
		</script>
		<script defer="defer" nonce="${nonce}" src="${scriptUri}"></script>
	</head>
	<body>
		<div id="root" class="${extraRootClass}">${loadingText}</div>
	</body>
	</html>`;
}

function getNonce(): string {
  return crypto.randomBytes(16).toString('base64');
}
