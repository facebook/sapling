/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {VSCodeServerPlatform} from './vscodePlatform';
import type {Logger} from 'isl-server/src/logger';
import type {ServerPlatform} from 'isl-server/src/serverPlatform';
import type {AppMode, ClientToServerMessage, ServerToClientMessage} from 'isl/src/types';
import type {Comparison} from 'shared/Comparison';

import packageJson from '../package.json';
import {executeVSCodeCommand} from './commands';
import {getCLICommand, PERSISTED_STORAGE_KEY_PREFIX, shouldOpenBeside} from './config';
import {locale, t} from './i18n';
import {onClientConnection} from 'isl-server/src';
import {deserializeFromString, serializeToString} from 'isl/src/serialize';
import crypto from 'node:crypto';
import {ComparisonType, isComparison, labelForComparison} from 'shared/Comparison';
import {nullthrows} from 'shared/utils';
import * as vscode from 'vscode';

let islPanelOrView: vscode.WebviewPanel | vscode.WebviewView | undefined = undefined;
let hasOpenedISLWebviewBeforeState = false;

const islViewType = 'sapling.isl';
const comparisonViewType = 'sapling.comparison';

const devPort = 3005;
const devUri = `http://localhost:${devPort}`;

function createOrFocusISLWebview(
  context: vscode.ExtensionContext,
  platform: VSCodeServerPlatform,
  logger: Logger,
  column?: vscode.ViewColumn,
): vscode.WebviewPanel | vscode.WebviewView {
  // Try to re-use existing ISL panel/view
  if (islPanelOrView) {
    isPanel(islPanelOrView) ? islPanelOrView.reveal() : islPanelOrView.show();
    return islPanelOrView;
  }
  // Otherwise, create a new panel/view

  const viewColumn = column ?? vscode.window.activeTextEditor?.viewColumn ?? vscode.ViewColumn.One;

  islPanelOrView = populateAndSetISLWebview(
    context,
    vscode.window.createWebviewPanel(
      islViewType,
      t('isl.title'),
      viewColumn,
      getWebviewOptions(context),
    ),
    platform,
    {mode: 'isl'},
    logger,
  );
  return nullthrows(islPanelOrView);
}

function createComparisonWebview(
  context: vscode.ExtensionContext,
  platform: VSCodeServerPlatform,
  comparison: Comparison,
  logger: Logger,
): vscode.WebviewPanel {
  // always create a new comparison webview
  const column =
    shouldOpenBeside() && islPanelOrView != null && isPanel(islPanelOrView) && islPanelOrView.active
      ? vscode.ViewColumn.Beside
      : vscode.window.activeTextEditor?.viewColumn ?? vscode.ViewColumn.One;

  const webview = populateAndSetISLWebview(
    context,
    vscode.window.createWebviewPanel(
      comparisonViewType,
      labelForComparison(comparison),
      column,
      getWebviewOptions(context),
    ),
    platform,
    {mode: 'comparison', comparison},
    logger,
  );
  return webview;
}

function getWebviewOptions(
  context: vscode.ExtensionContext,
): vscode.WebviewOptions & vscode.WebviewPanelOptions {
  return {
    enableScripts: true,
    retainContextWhenHidden: true,
    // Restrict the webview to only loading content from our extension's `webview` directory.
    localResourceRoots: [
      vscode.Uri.joinPath(context.extensionUri, 'dist/webview'),
      vscode.Uri.parse(devUri),
    ],
    portMapping: [{webviewPort: devPort, extensionHostPort: devPort}],
  };
}

function shouldUseWebviewView(): boolean {
  return vscode.workspace.getConfiguration('sapling.isl').get<boolean>('showInSidebar') ?? false;
}

export function hasOpenedISLWebviewBefore() {
  return hasOpenedISLWebviewBeforeState;
}

/**
 * If a vscode extension host is restarted while ISL is open, the connection to the webview is severed.
 * If we activate and see pre-existing ISLs, we should either destroy them,
 * or open a fresh ISL in their place.
 * You might expect deserialization to handle this, but it doesn't.
 * See: https://github.com/microsoft/vscode/issues/188257
 */
function replaceExistingOrphanedISLWindows(
  context: vscode.ExtensionContext,
  platform: VSCodeServerPlatform,
  logger: Logger,
) {
  const orphanedTabs = vscode.window.tabGroups.all
    .flatMap(tabGroup => tabGroup.tabs)
    .filter(tab => (tab.input as vscode.TabInputWebview)?.viewType?.includes(islViewType));
  logger.info(`Found ${orphanedTabs.length} orphaned ISL tabs`);
  if (orphanedTabs.length > 0) {
    for (const tab of orphanedTabs) {
      // We only remake the ISL tab if it's active, since otherwise it will focus it.
      // The exception is if you had ISL pinned, since your pin would get destroyed which is annoying.
      // It does mean that the pinned ISL steals focus, but I think that's reasonable during an exthost restart.
      if ((tab.isActive || tab.isPinned) && !shouldUseWebviewView()) {
        // Make sure we use the matching ViewColumn so it feels like we recreate ISL in the same place.
        const {viewColumn} = tab.group;
        logger.info(` > Replacing orphaned ISL with fresh one for view column ${viewColumn}`);
        try {
          // We only expect there to be at most one "active" tab, but even if there were,
          // this command would still reuse the existing ISL.
          createOrFocusISLWebview(context, platform, logger, viewColumn);
        } catch (err: unknown) {
          vscode.window.showErrorMessage(`error opening isl: ${err}`);
        }

        if (tab.isPinned) {
          executeVSCodeCommand('workbench.action.pinEditor');
        }
      }
      // Regardless of if we opened a new ISL, reap the old one. It wouldn't work if you clicked on it.
      vscode.window.tabGroups.close(orphanedTabs);
    }
  }
}

export function registerISLCommands(
  context: vscode.ExtensionContext,
  platform: VSCodeServerPlatform,
  logger: Logger,
): vscode.Disposable {
  const webviewViewProvider = new ISLWebviewViewProvider(context, platform, logger);

  replaceExistingOrphanedISLWindows(context, platform, logger);

  const createComparisonWebviewCommand = (comparison: Comparison) => {
    try {
      createComparisonWebview(context, platform, comparison, logger);
    } catch (err: unknown) {
      vscode.window.showErrorMessage(
        `error opening ${labelForComparison(comparison)} comparison: ${err}`,
      );
    }
  };
  return vscode.Disposable.from(
    vscode.commands.registerCommand('sapling.open-isl', () => {
      if (shouldUseWebviewView()) {
        // just open the sidebar view
        executeVSCodeCommand('sapling.isl.focus');
        return;
      }
      try {
        createOrFocusISLWebview(context, platform, logger);
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
    vscode.commands.registerCommand('sapling.open-comparison-view-uncommitted', () => {
      createComparisonWebviewCommand({type: ComparisonType.UncommittedChanges});
    }),
    vscode.commands.registerCommand('sapling.open-comparison-view-head', () => {
      createComparisonWebviewCommand({type: ComparisonType.HeadChanges});
    }),
    vscode.commands.registerCommand('sapling.open-comparison-view-stack', () => {
      createComparisonWebviewCommand({type: ComparisonType.StackChanges});
    }),
    /** Command that opens the provided Comparison argument. Intended to be used programmatically. */
    vscode.commands.registerCommand('sapling.open-comparison-view', (comparison: unknown) => {
      if (!isComparison(comparison)) {
        return;
      }
      createComparisonWebviewCommand(comparison);
    }),
    registerDeserializer(context, platform, logger),
    vscode.window.registerWebviewViewProvider(islViewType, webviewViewProvider, {
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

function registerDeserializer(
  context: vscode.ExtensionContext,
  platform: VSCodeServerPlatform,
  logger: Logger,
) {
  // Make sure we register a serializer in activation event
  return vscode.window.registerWebviewPanelSerializer(islViewType, {
    deserializeWebviewPanel(webviewPanel: vscode.WebviewPanel, _state: unknown) {
      if (shouldUseWebviewView()) {
        // if we try to deserialize a panel while we're trying to use view, destroy the panel and open the sidebar instead
        webviewPanel.dispose();
        executeVSCodeCommand('sapling.isl.focus');
        return Promise.resolve();
      }
      // Reset the webview options so we use latest uri for `localResourceRoots`.
      webviewPanel.webview.options = getWebviewOptions(context);
      populateAndSetISLWebview(context, webviewPanel, platform, {mode: 'isl'}, logger);
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
  constructor(
    private extensionContext: vscode.ExtensionContext,
    private platform: VSCodeServerPlatform,
    private logger: Logger,
  ) {}

  resolveWebviewView(webviewView: vscode.WebviewView): void | Thenable<void> {
    webviewView.webview.options = getWebviewOptions(this.extensionContext);
    populateAndSetISLWebview(
      this.extensionContext,
      webviewView,
      this.platform,
      {mode: 'isl'},
      this.logger,
    );
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
  platform: VSCodeServerPlatform,
  mode: AppMode,
  logger: Logger,
): W {
  logger.info(`Populating ISL webview ${isPanel(panelOrView) ? 'panel' : 'view'}`);
  hasOpenedISLWebviewBeforeState = true;
  if (mode.mode === 'isl' && isPanel(panelOrView)) {
    islPanelOrView = panelOrView;
  }
  if (isPanel(panelOrView)) {
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
    mode,
    logger,
  );
  const updatedPlatform = {...platform, panelOrView} as VSCodeServerPlatform as ServerPlatform;

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
    platform: updatedPlatform,
    appMode: mode,
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
          if (data.type === 'gotUiState') {
            dispose?.dispose();
            dispose = undefined;
            resolve({state: data.state});
          }
        } catch {}
      },
    );

    islPanelOrView?.webview.postMessage(
      serializeToString({type: 'getUiState'} as ServerToClientMessage),
    );
  });
}

/**
 * To persist state, we store data in extension globalStorage.
 * In order to access this synchronously at startup inside the webview,
 * we need to inject this initial state into the webview HTML.
 * This gives the javascript snippet that can be safely put into a webview HTML <script> tag.
 */
function getInitialStateJs(context: vscode.ExtensionContext, logger: Logger) {
  // Previously, all state was stored in a single global storage key.
  // This meant we read and wrote the entire state on every change,
  // notably the webview sent the entire state to the extension on every change.
  // Now, we store each piece of state in its own key, and only send the changed keys to the extension.

  const legacyKey = 'isl-persisted';

  const legacyStateStr = context.globalState.get<string>(legacyKey);
  let parsed: {[key: string]: unknown};
  if (legacyStateStr != null) {
    // We migrate to the new system if we see data in the old key.
    // This can be deleted after some time to let clients update.
    logger.info('Legacy persisted state format found, migrating to individual keys');

    try {
      parsed = JSON.parse(legacyStateStr);

      // This snippet is injected directly as javascript, much like `eval`.
      // Therefore, it's very important that the stateStr is validated to be safe to be injected.
      if (typeof parsed !== 'object' || parsed == null) {
        // JSON is not in the format we expect
        logger.info('Found INVALID JSON for initial persisted state for webview: ', legacyStateStr);
        // Move forward with empty data (eventually deleting the legacy key)
        parsed = {};
      }

      for (const key in parsed) {
        context.globalState.update(PERSISTED_STORAGE_KEY_PREFIX + key, parsed[key]);
      }
      logger.info(`Migrated ${Object.keys(parsed).length} keys from legacy persisted state`);
    } catch {
      logger.info('Found INVALID (legacy) initial persisted state for webview: ', legacyStateStr);
      return '';
    } finally {
      // Delete the legacy data either way
      context.globalState.update(legacyKey, undefined);
      logger.info('Deleted legacy persisted state');
    }
  } else {
    logger.info('No legacy persisted state found');

    const allDataKeys = context.globalState.keys();
    parsed = {};

    for (const fullKey of allDataKeys) {
      if (fullKey.startsWith(PERSISTED_STORAGE_KEY_PREFIX)) {
        const keyWithoutPrefix = fullKey.slice(PERSISTED_STORAGE_KEY_PREFIX.length);
        const found = context.globalState.get<string>(fullKey);
        if (found) {
          try {
            parsed[keyWithoutPrefix] = JSON.parse(found);
          } catch (err) {
            logger.error(
              `Failed to parse persisted state for key ${keyWithoutPrefix}. Skipping. ${err}`,
            );
          }
        }
      }
    }

    logger.info(`Loaded persisted data for ${allDataKeys.length} keys`);
  }

  try {
    // validated is injected not as a string, but directly as a javascript object (since JSON is a subset of js)
    const validated = JSON.stringify(parsed);
    logger.info('Found valid initial persisted state for webview: ', validated);
    return `try {
      window.islInitialPersistedState = ${validated};
    } catch (e) {}
    `;
  } catch {
    logger.info('Found INVALID initial persisted state for webview: ', parsed);
    return '';
  }
}

/**
 * Get any extra styles to inject into the webview.
 * Important: this is injected into the HTML directly, and should not
 * use any user-controlled data that could be used maliciously.
 */
function getExtraStyles(): string {
  const globalStyles = new Map();

  const fontFeatureSettings = vscode.workspace
    .getConfiguration('editor')
    .get<string | boolean>('fontLigatures');
  const validFontFeaturesRegex = /^[0-9a-zA-Z"',\-_ ]*$/;
  if (fontFeatureSettings === true) {
    // no need to specify specific additional settings
  } else if (
    !fontFeatureSettings ||
    typeof fontFeatureSettings !== 'string' ||
    !validFontFeaturesRegex.test(fontFeatureSettings)
  ) {
    globalStyles.set('font-variant-ligatures', 'none');
  } else {
    globalStyles.set('font-feature-settings', fontFeatureSettings);
  }

  const tabSizeSettings = vscode.workspace.getConfiguration('editor').get<number>('tabSize');
  if (typeof tabSizeSettings === 'number') {
    globalStyles.set('--tab-size', tabSizeSettings);
  }

  const globalStylesFlat = Array.from(globalStyles, ([k, v]) => `${k}: ${v};`);
  return `
  html {
    ${globalStylesFlat.join('\n')};
  }`;
}

/**
 * When built in dev mode using vite, files are not written to disk.
 * In order to get files to load, we need to set up the server path ourself.
 *
 * Note: no CSPs in dev mode. This should not be used in production!
 */
function devModeHtmlForISLWebview(
  kind: 'panel' | 'view',
  context: vscode.ExtensionContext,
  appMode: AppMode,
  logger: Logger,
) {
  logger.info('using dev mode webview');
  // make resource access use vite dev server, instead of `webview.asWebviewUri`
  const baseUri = vscode.Uri.parse(devUri);

  const extraRootClass = `webview-${kind}`;

  return `<!DOCTYPE html>
	<html lang="en">
	<head>
		<meta charset="UTF-8">
		<meta name="viewport" content="width=device-width, initial-scale=1.0">
		<base href="${baseUri}">

    <!-- Hot reloading code from Vite. Normally, vite injects this into the HTML.
    But since we have to load this statically, we insert it manually here.
    See https://github.com/vitejs/vite/blob/734a9e3a4b9a0824a5ba4a5420f9e1176ce74093/docs/guide/backend-integration.md?plain=1#L50-L56 -->
    <script type="module">
      import RefreshRuntime from "/@react-refresh"
      RefreshRuntime.injectIntoGlobalHook(window)
      window.$RefreshReg$ = () => {}
      window.$RefreshSig$ = () => (type) => type
      window.__vite_plugin_react_preamble_installed__ = true
    </script>
    <script type="module" src="/@vite/client"></script>
    <style>
      ${getExtraStyles()}
    </style>

		<script>
			window.saplingLanguage = "${locale /* important: locale has already been validated */}";
      window.islAppMode = ${JSON.stringify(appMode)};
      ${getInitialStateJs(context, logger)}
		</script>
    <script type="module" src="/webview/islWebviewPreload.ts"></script>
    <script type="module" src="/webview/islWebviewEntry.tsx"></script>
	</head>
	<body>
		<div id="root" class="${extraRootClass}">loading (dev mode)</div>
	</body>
	</html>`;
}

const IS_DEV_BUILD = process.env.NODE_ENV === 'development';
function htmlForISLWebview(
  context: vscode.ExtensionContext,
  webview: vscode.Webview,
  kind: 'panel' | 'view',
  appMode: AppMode,
  logger: Logger,
) {
  if (IS_DEV_BUILD) {
    return devModeHtmlForISLWebview(kind, context, appMode, logger);
  }

  // Only allow accessing resources relative to webview dir,
  // and make paths relative to here.
  const baseUri = webview.asWebviewUri(
    vscode.Uri.joinPath(context.extensionUri, 'dist', 'webview'),
  );

  const scriptUri = 'webview.js';

  // Use a nonce to only allow specific scripts to be run
  const nonce = getNonce();

  const loadingText = t('isl.loading-text');
  const titleText = t('isl.title');

  const extraRootClass = `webview-${kind}`;

  const CSP = [
    `default-src ${webview.cspSource}`,
    `style-src ${webview.cspSource} 'unsafe-inline'`,
    // vscode-webview-ui needs to use style-src-elem without the nonce
    `style-src-elem ${webview.cspSource} 'unsafe-inline'`,
    `font-src ${webview.cspSource} data:`,
    `img-src ${webview.cspSource} https: data:`,
    `script-src ${webview.cspSource} 'nonce-${nonce}' 'wasm-unsafe-eval'`,
    `script-src-elem ${webview.cspSource} 'nonce-${nonce}'`,
    `worker-src ${webview.cspSource} 'nonce-${nonce}' blob:`,
  ].join('; ');

  return `<!DOCTYPE html>
	<html lang="en">
	<head>
		<meta charset="UTF-8">
		<meta http-equiv="Content-Security-Policy" content="${CSP}">
		<meta name="viewport" content="width=device-width, initial-scale=1.0">
		<base href="${baseUri}/">
		<title>${titleText}</title>
		<link href="res/webview.css" rel="stylesheet">
		<link href="res/stylex.css" rel="stylesheet">
    <style>
      ${getExtraStyles()}
    </style>
		<script nonce="${nonce}">
			window.saplingLanguage = "${locale /* important: locale has already been validated */}";
      window.islAppMode = ${JSON.stringify(appMode)};
      ${getInitialStateJs(context, logger)}
		</script>
		<script type="module" defer="defer" nonce="${nonce}" src="${scriptUri}"></script>
	</head>
	<body>
		<div id="root" class="${extraRootClass}">${loadingText}</div>
	</body>
	</html>`;
}

function getNonce(): string {
  return crypto.randomBytes(16).toString('base64');
}
