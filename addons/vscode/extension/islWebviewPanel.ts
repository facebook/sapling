/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from 'isl-server/src/logger';
import type {ServerPlatform} from 'isl-server/src/serverPlatform';
import type {AppMode, ClientToServerMessage, ServerToClientMessage} from 'isl/src/types';
import type {Comparison} from 'shared/Comparison';
import type {WebviewPanel, WebviewView} from 'vscode';
import type {VSCodeServerPlatform} from './vscodePlatform';

/**
 * Interface representing the result of creating or focusing an ISL webview.
 * Contains both the panel/view and a promise that resolves when the client is ready.
 */
interface ISLWebviewResult<W extends WebviewPanel | WebviewView> {
  panel: W;
  readySignal: Deferred<void>;
}

import {onClientConnection} from 'isl-server/src';
import {deserializeFromString, serializeToString} from 'isl/src/serialize';
import type {PartiallySelectedDiffCommit} from 'isl/src/stackEdit/diffSplitTypes';
import {ComparisonType, isComparison, labelForComparison} from 'shared/Comparison';
import type {Deferred} from 'shared/utils';
import {defer} from 'shared/utils';
import * as vscode from 'vscode';
import {executeVSCodeCommand} from './commands';
import {getCLICommand, PERSISTED_STORAGE_KEY_PREFIX, shouldOpenBeside} from './config';
import {getWebviewOptions, htmlForWebview} from './htmlForWebview';
import {locale, t} from './i18n';
import {extensionVersion} from './utils';

/**
 * Expands line ranges to individual line numbers.
 * Input is ALWAYS ranges (from AI agent output), never individual line numbers.
 *
 * Supported formats:
 * 1. Array of ranges: [[0, 100], [150, 200]] -> [0,1,2,...,100,150,151,...,200]
 * 2. Single range: [0, 100] -> [0,1,2,...,100]
 *
 * Ranges are inclusive on both ends: [0, 161] expands to lines 0 through 161.
 */
function expandLineRange(
  lines: ReadonlyArray<number> | ReadonlyArray<[number, number]>,
): ReadonlyArray<number> {
  if (lines.length === 0) {
    return [];
  }

  const expanded: number[] = [];

  // Check if it's an array of ranges: [[start, end], [start, end], ...]
  if (Array.isArray(lines[0])) {
    for (const range of lines as ReadonlyArray<[number, number]>) {
      if (Array.isArray(range) && range.length === 2) {
        const [start, end] = range;
        if (typeof start === 'number' && typeof end === 'number') {
          // Expand the range inclusively: [start, end] -> [start, start+1, ..., end]
          for (let i = start; i <= end; i++) {
            expanded.push(i);
          }
        }
      }
    }
    return expanded;
  }

  // Single range format: [start, end]
  // This MUST be a 2-element array representing a range
  if (lines.length === 2) {
    const [start, end] = lines as [number, number];
    if (typeof start === 'number' && typeof end === 'number') {
      // Expand the range inclusively: [start, end] -> [start, start+1, ..., end]
      for (let i = start; i <= end; i++) {
        expanded.push(i);
      }
      return expanded;
    }
  }

  // If we get here with lines.length !== 2, the input format is unexpected.
  // This shouldn't happen with proper agent output - log a warning.
  console.warn(
    `expandLineRange received unexpected format with ${lines.length} elements. Expected a range [start, end] or array of ranges.`,
  );
  return lines as ReadonlyArray<number>;
}

let islPanelOrViewResult: ISLWebviewResult<vscode.WebviewPanel | vscode.WebviewView> | undefined =
  undefined;
let hasOpenedISLWebviewBeforeState = false;

const islViewType = 'sapling.isl';
const comparisonViewType = 'sapling.comparison';

/**
 * Creates or focuses the ISL webview and returns both the panel/view and a promise that resolves when the client is ready.
 */
function createOrFocusISLWebview(
  context: vscode.ExtensionContext,
  platform: VSCodeServerPlatform,
  logger: Logger,
  column?: vscode.ViewColumn,
): ISLWebviewResult<vscode.WebviewPanel | vscode.WebviewView> {
  // Try to reuse existing ISL panel/view
  if (islPanelOrViewResult) {
    isPanel(islPanelOrViewResult.panel)
      ? islPanelOrViewResult.panel.reveal()
      : islPanelOrViewResult.panel.show();
    return islPanelOrViewResult;
  }
  // Otherwise, create a new panel/view

  const viewColumn = column ?? vscode.window.activeTextEditor?.viewColumn ?? vscode.ViewColumn.One;

  islPanelOrViewResult = populateAndSetISLWebview(
    context,
    vscode.window.createWebviewPanel(
      islViewType,
      t('isl.title'),
      viewColumn,
      getWebviewOptions(context, 'dist/webview'),
    ),
    platform,
    {mode: 'isl'},
    logger,
  );

  return islPanelOrViewResult;
}

function createComparisonWebview(
  context: vscode.ExtensionContext,
  platform: VSCodeServerPlatform,
  comparison: Comparison,
  logger: Logger,
): ISLWebviewResult<vscode.WebviewPanel> {
  // always create a new comparison webview
  const column =
    shouldOpenBeside() &&
    islPanelOrViewResult != null &&
    isPanel(islPanelOrViewResult.panel) &&
    islPanelOrViewResult.panel.active
      ? vscode.ViewColumn.Beside
      : (vscode.window.activeTextEditor?.viewColumn ?? vscode.ViewColumn.One);

  const webview = populateAndSetISLWebview(
    context,
    vscode.window.createWebviewPanel(
      comparisonViewType,
      labelForComparison(comparison),
      column,
      getWebviewOptions(context, 'dist/webview'),
    ),
    platform,
    {mode: 'comparison', comparison},
    logger,
  );
  return webview;
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
    vscode.commands.registerCommand(
      'sapling.open-isl-with-commit-message',
      async (title: string, description: string, mode?: 'commit' | 'amend', hash?: string) => {
        try {
          let readySignal: Deferred<void>;

          if (shouldUseWebviewView()) {
            executeVSCodeCommand('sapling.isl.focus');
            // For webview views, use the readySignal from the provider
            readySignal = webviewViewProvider.readySignal;
          } else {
            const result = createOrFocusISLWebview(context, platform, logger);
            readySignal = result.readySignal;
          }

          await readySignal.promise;

          const currentPanelOrViewResult = islPanelOrViewResult;
          if (currentPanelOrViewResult) {
            const message: ServerToClientMessage = {
              type: 'updateDraftCommitMessage',
              title,
              description,
              mode,
              hash,
            };

            currentPanelOrViewResult.panel.webview.postMessage(serializeToString(message));
          }
        } catch (err: unknown) {
          vscode.window.showErrorMessage(`Error opening ISL with commit message: ${err}`);
        }
      },
    ),
    vscode.commands.registerCommand(
      'sapling.open-split-view-with-commits',
      async (commits: Array<PartiallySelectedDiffCommit>, commitHash?: string) => {
        try {
          let readySignal: Deferred<void>;

          if (shouldUseWebviewView()) {
            executeVSCodeCommand('sapling.isl.focus');
            readySignal = webviewViewProvider.readySignal;
          } else {
            const result = createOrFocusISLWebview(context, platform, logger);
            readySignal = result.readySignal;
          }
          await readySignal.promise;

          const currentPanelOrViewResult = islPanelOrViewResult;
          if (currentPanelOrViewResult) {
            if (commitHash) {
              // Expand line ranges [start, end] to individual line numbers before sending
              const expandedCommits = commits.map(commit => ({
                ...commit,
                files: commit.files.map(file => ({
                  ...file,
                  aLines: expandLineRange(file.aLines),
                  bLines: expandLineRange(file.bLines),
                })),
              }));

              // Send a single message that opens the split view and applies commits after loading
              const openSplitMessage: ServerToClientMessage = {
                type: 'openSplitViewForCommit',
                commitHash,
                commits: expandedCommits,
              };
              currentPanelOrViewResult.panel.webview.postMessage(
                serializeToString(openSplitMessage),
              );
            } else {
              vscode.window.showErrorMessage(`Error opening split view: no commit hash provided`);
            }
          }
        } catch (err: unknown) {
          vscode.window.showErrorMessage(`Error opening split view: ${err}`);
        }
      },
    ),
    vscode.commands.registerCommand('sapling.close-isl', () => {
      if (!islPanelOrViewResult) {
        return;
      }
      if (isPanel(islPanelOrViewResult.panel)) {
        islPanelOrViewResult.panel.dispose();
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
        if (islPanelOrViewResult && isPanel(islPanelOrViewResult.panel) && shouldUseWebviewView()) {
          islPanelOrViewResult.panel.dispose();
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
      webviewPanel.webview.options = getWebviewOptions(context, 'dist/webview');
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
  // Signal that resolves when the webview view is ready
  public readySignal: Deferred<void> = defer<void>();

  constructor(
    private extensionContext: vscode.ExtensionContext,
    private platform: VSCodeServerPlatform,
    private logger: Logger,
  ) {}

  resolveWebviewView(webviewView: vscode.WebviewView): void | Thenable<void> {
    webviewView.webview.options = getWebviewOptions(this.extensionContext, 'dist/webview');
    const result = populateAndSetISLWebview(
      this.extensionContext,
      webviewView,
      this.platform,
      {mode: 'isl'},
      this.logger,
    );

    this.readySignal = result.readySignal;
  }
}

function isPanel(
  panelOrView: vscode.WebviewPanel | vscode.WebviewView,
): panelOrView is vscode.WebviewPanel {
  // panels have a .reveal property, views have .show
  return (panelOrView as vscode.WebviewPanel).reveal !== undefined;
}

/**
 * Populates and sets up an ISL webview panel or view.
 * Returns both the panel/view and a Deferred that resolves when the client signals it's ready.
 */
function populateAndSetISLWebview<W extends vscode.WebviewPanel | vscode.WebviewView>(
  context: vscode.ExtensionContext,
  panelOrView: W,
  platform: VSCodeServerPlatform,
  mode: AppMode,
  logger: Logger,
): ISLWebviewResult<W> {
  const readySignal = defer<void>();
  logger.info(`Populating ISL webview ${isPanel(panelOrView) ? 'panel' : 'view'}`);
  hasOpenedISLWebviewBeforeState = true;
  if (mode.mode === 'isl') {
    islPanelOrViewResult = {panel: panelOrView, readySignal};
  }
  if (isPanel(panelOrView)) {
    panelOrView.iconPath = vscode.Uri.joinPath(
      context.extensionUri,
      'resources',
      'Sapling_favicon-light-green-transparent.svg',
    );
  }
  panelOrView.webview.html = htmlForWebview({
    context,
    extensionRelativeBase: 'dist/webview',
    entryPointFile: 'webview.js',
    cssEntryPointFile: 'res/style.css', // TODO: this is global to all webviews, but should instead be per webview
    devModeScripts: ['/webview/islWebviewEntry.tsx'],
    title: t('isl.title'),
    rootClass: `webview-${isPanel(panelOrView) ? 'panel' : 'view'}`,
    webview: panelOrView.webview,
    extraStyles: '',
    initialScript: nonce => `
    <script nonce="${nonce}" type="text/javascript">
      window.saplingLanguage = "${locale /* important: locale has already been validated */}";
      window.islAppMode = ${JSON.stringify(mode)};
    </script>
    ${getInitialStateJs(context, logger, nonce)}
    `,
  });
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
    version: extensionVersion,
    readySignal,
  });

  panelOrView.onDidDispose(() => {
    if (isPanel(panelOrView)) {
      logger.info('Disposing ISL panel');
      islPanelOrViewResult = undefined;
    } else {
      logger.info('Disposing ISL view');
    }
    disposeConnection();
  });

  return {panel: panelOrView, readySignal};
}

export function fetchUIState(): Promise<{state: string} | undefined> {
  if (islPanelOrViewResult == null) {
    return Promise.resolve(undefined);
  }

  return new Promise(resolve => {
    let dispose: vscode.Disposable | undefined =
      islPanelOrViewResult?.panel.webview.onDidReceiveMessage((m: string) => {
        try {
          const data = deserializeFromString(m) as ClientToServerMessage;
          if (data.type === 'gotUiState') {
            dispose?.dispose();
            dispose = undefined;
            resolve({state: data.state});
          }
        } catch {}
      });

    islPanelOrViewResult?.panel.webview.postMessage(
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
function getInitialStateJs(context: vscode.ExtensionContext, logger: Logger, nonce: string) {
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
    // validated is injected not as a string, but directly as a javascript object into a dedicated tag
    const validated = JSON.stringify(parsed);
    const escaped = validated.replace(/</g, '\\u003c');
    logger.info('Found valid initial persisted state for webview: ', validated);
    return `
    <script type="application/json" id="isl-persisted-state">
      ${escaped}
    </script>
    <script nonce="${nonce}" type="text/javascript">
      try {
          const stateElement = document.getElementById('isl-persisted-state');
          window.islInitialPersistedState = JSON.parse(stateElement.textContent);
        } catch (e) {
          console.error('Failed to parse initial persisted state: ', e);
          window.islInitialPersistedState = {};
        }
     </script>
    `;
  } catch {
    logger.info('Found INVALID initial persisted state for webview: ', parsed);
    return '';
  }
}
