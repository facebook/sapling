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
import {repositoryCache} from 'isl-server/src/RepositoryCache';
import {deserializeFromString, serializeToString} from 'isl/src/serialize';
import type {PartiallySelectedDiffCommit} from 'isl/src/stackEdit/diffSplitTypes';
import {ComparisonType, isComparison, labelForComparison} from 'shared/Comparison';
import type {Deferred} from 'shared/utils';
import {defer} from 'shared/utils';
import * as vscode from 'vscode';
import {executeVSCodeCommand} from './commands';
import {getCLICommand, PERSISTED_STORAGE_KEY_PREFIX, shouldOpenBeside} from './config';
import {assignWebviewHtml, getWebviewOptions} from './htmlForWebview';
import {locale, t} from './i18n';
import {Internal} from './Internal';
import {hasMultiDiffEditorSupport, openMultiDiffEditor} from './multiDiffEditor';
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
  // eslint-disable-next-line no-console -- intentional warning for unexpected input
  console.warn(
    `expandLineRange received unexpected format with ${lines.length} elements. Expected a range [start, end] or array of ranges.`,
  );
  return lines as ReadonlyArray<number>;
}

let islPanelOrViewResult: ISLWebviewResult<vscode.WebviewPanel | vscode.WebviewView> | undefined =
  undefined;
let hasOpenedISLWebviewBeforeState = false;

/** Most recently selected cwd across all ISL webviews. */
let mostRecentISLCwd: string | undefined = undefined;

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
  cwd?: string,
): ISLWebviewResult<vscode.WebviewPanel | vscode.WebviewView> {
  // Try to reuse existing ISL panel/view
  if (islPanelOrViewResult) {
    isPanel(islPanelOrViewResult.panel)
      ? islPanelOrViewResult.panel.reveal()
      : islPanelOrViewResult.panel.show();
    // The single shared webview may be showing a different repo; switch it to the requested one.
    if (cwd != null) {
      postMessageToISLWebview({type: 'changeActiveRepo', cwd, focusDotCommit: true});
    }
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
    cwd,
  );

  return islPanelOrViewResult;
}

/**
 * `sapling.open-isl` can be triggered from a repository's SCM title bar, an editor title
 * button, a keybinding, or programmatically. Resolve which repository's cwd the command
 * should target from whatever argument VS Code passes for those entry points. Returns
 * undefined when there's no repo-specific context (e.g. the keybinding), in which case the
 * default cwd selection applies.
 */
export function cwdForOpenISLCommand(arg: unknown): string | undefined {
  // The scm/title menu passes the repository's vscode.SourceControl, whose rootUri is the repo root.
  const maybeSourceControl = arg as {rootUri?: vscode.Uri} | undefined;
  if (maybeSourceControl?.rootUri instanceof vscode.Uri) {
    return maybeSourceControl.rootUri.fsPath;
  }
  // The editor/title menu passes the active file's Uri; map it back to its repo root.
  if (arg instanceof vscode.Uri) {
    return repositoryCache.cachedRepositoryForPath(arg.fsPath)?.info.repoRoot ?? arg.fsPath;
  }
  return undefined;
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
  // `tabGroups` is window-global, but in Basecamp one window hosts many extension hosts -- so this
  // would reclaim another host's ISL tab. Basecamp drives ISL lifecycle itself, so skip recovery.
  if (Internal.isBasecamp?.()) {
    return;
  }
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

/**
 * Opens the native VS Code multi-diff editor for a comparison using Sapling's
 * own file status fetching and content providers. Returns true if successful,
 * false if it falls back to the webview comparison.
 */
async function openNativeMultiDiffEditor(
  comparison: Comparison,
  repoRoot?: string,
): Promise<boolean> {
  if (!(await hasMultiDiffEditorSupport())) {
    return false;
  }

  // Resolve repo: explicit repoRoot -> active editor file -> ISL's selected cwd -> workspace folders.
  let repo;
  if (repoRoot) {
    repo = repositoryCache.cachedRepositoryForPath(repoRoot);
  }
  if (!repo) {
    const activeUri = vscode.window.activeTextEditor?.document.uri;
    if (activeUri && activeUri.scheme === 'file') {
      repo = repositoryCache.cachedRepositoryForPath(activeUri.fsPath);
    }
  }
  if (!repo && mostRecentISLCwd) {
    repo = repositoryCache.cachedRepositoryForPath(mostRecentISLCwd);
  }
  if (!repo) {
    for (const folder of vscode.workspace.workspaceFolders ?? []) {
      repo = repositoryCache.cachedRepositoryForPath(folder.uri.fsPath);
      if (repo) {
        break;
      }
    }
  }
  if (!repo) {
    return false;
  }

  try {
    const files = await repo.getFilesChangedForComparison(
      repo.initialConnectionContext,
      comparison,
    );
    if (files.length === 0) {
      vscode.window.showInformationMessage(t('No changed files to display'));
      return true; // Handled, just nothing to show
    }

    await openMultiDiffEditor(repo.info.repoRoot, comparison, files);
    return true;
  } catch (err) {
    // If multi-diff editor fails, fall back to webview
    return false;
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
    vscode.commands.registerCommand('sapling.open-isl', (arg?: unknown) => {
      const cwd = cwdForOpenISLCommand(arg);
      if (shouldUseWebviewView()) {
        const viewExists = islPanelOrViewResult != null && !isPanel(islPanelOrViewResult.panel);
        if (viewExists) {
          // The view already exists, so `resolveWebviewView` won't run again; reveal it and
          // switch the existing webview to the requested repo synchronously.
          executeVSCodeCommand('sapling.isl.focus');
          if (cwd != null) {
            postMessageToISLWebview({type: 'changeActiveRepo', cwd, focusDotCommit: true});
          }
        } else {
          // The view hasn't been created yet; `sapling.isl.focus` triggers `resolveWebviewView`
          // asynchronously. Thread the cwd through the provider so the initial populate targets
          // the requested repo, since a synchronous `postMessageToISLWebview` would no-op here.
          webviewViewProvider.setInitialCwd(cwd);
          executeVSCodeCommand('sapling.isl.focus');
        }
        return;
      }
      try {
        createOrFocusISLWebview(context, platform, logger, undefined, cwd);
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
    vscode.commands.registerCommand('sapling.open-comparison-view-uncommitted', async () => {
      const comparison: Comparison = {type: ComparisonType.UncommittedChanges};
      if (!(await openNativeMultiDiffEditor(comparison))) {
        createComparisonWebviewCommand(comparison);
      }
    }),
    vscode.commands.registerCommand('sapling.open-comparison-view-head', async () => {
      const comparison: Comparison = {type: ComparisonType.HeadChanges};
      if (!(await openNativeMultiDiffEditor(comparison))) {
        createComparisonWebviewCommand(comparison);
      }
    }),
    vscode.commands.registerCommand('sapling.open-comparison-view-stack', async () => {
      const comparison: Comparison = {type: ComparisonType.StackChanges};
      if (!(await openNativeMultiDiffEditor(comparison))) {
        createComparisonWebviewCommand(comparison);
      }
    }),
    /** Command that opens the provided Comparison argument. Intended to be used programmatically. */
    vscode.commands.registerCommand(
      'sapling.open-comparison-view',
      async (comparison: unknown, repoRoot?: string) => {
        if (!isComparison(comparison)) {
          return;
        }
        if (!(await openNativeMultiDiffEditor(comparison, repoRoot))) {
          createComparisonWebviewCommand(comparison);
        }
      },
    ),
    registerDeserializer(context, platform, logger),
    vscode.window.registerWebviewViewProvider(islViewType, webviewViewProvider, {
      webviewOptions: {
        retainContextWhenHidden: true,
      },
    }),
    vscode.workspace.onDidChangeConfiguration(e => {
      if (e.affectsConfiguration('sapling.isl.showInSidebar')) {
        if (shouldUseWebviewView()) {
          // Switching to sidebar mode: dispose the panel if it exists
          if (islPanelOrViewResult && isPanel(islPanelOrViewResult.panel)) {
            islPanelOrViewResult.panel.dispose();
          }
          executeVSCodeCommand('sapling.isl.focus');
        } else {
          // Switching to panel mode: clear the view reference so a new panel can be created
          if (islPanelOrViewResult && !isPanel(islPanelOrViewResult.panel)) {
            islPanelOrViewResult = undefined;
          }
          createOrFocusISLWebview(context, platform, logger);
        }
      }
    }),
    Internal.basecampOnDidChangeFocusedEnvironment?.(
      (env?: {folderPaths: ReadonlyArray<string>}) => {
        if (env?.folderPaths?.[0]) {
          postMessageToISLWebview({
            type: 'changeActiveRepo',
            cwd: env.folderPaths[0],
            focusDotCommit: true,
          });
        }
      },
    ) ?? new vscode.Disposable(() => {}),
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

  /** cwd to use the next time the view is (re)created, e.g. when opened for a specific repo. */
  private initialCwd: string | undefined = undefined;

  constructor(
    private extensionContext: vscode.ExtensionContext,
    private platform: VSCodeServerPlatform,
    private logger: Logger,
  ) {}

  setInitialCwd(cwd: string | undefined): void {
    this.initialCwd = cwd;
  }

  resolveWebviewView(webviewView: vscode.WebviewView): void | Thenable<void> {
    webviewView.webview.options = getWebviewOptions(this.extensionContext, 'dist/webview');
    const result = populateAndSetISLWebview(
      this.extensionContext,
      webviewView,
      this.platform,
      {mode: 'isl'},
      this.logger,
      this.initialCwd,
    );
    this.initialCwd = undefined;

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
  cwd?: string,
): ISLWebviewResult<W> {
  const readySignal = defer<void>();
  logger.info(`Populating ISL webview ${isPanel(panelOrView) ? 'panel' : 'view'}`);
  hasOpenedISLWebviewBeforeState = true;
  if (mode.mode === 'isl') {
    islPanelOrViewResult = {panel: panelOrView, readySignal};
  }
  if (isPanel(panelOrView)) {
    panelOrView.iconPath = {
      light: vscode.Uri.joinPath(context.extensionUri, 'resources', 'Sapling-light.svg'),
      dark: vscode.Uri.joinPath(context.extensionUri, 'resources', 'Sapling-dark.svg'),
    };
  }
  assignWebviewHtml({
    context,
    extensionRelativeBase: 'dist/webview',
    entryPointFile: 'webview.js',
    cssEntryPointFile: 'res/style.css', // TODO: this is global to all webviews, but should instead be per webview
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

  const focusedEnv = Internal.basecampGetFocusedEnvironment?.();
  const initialCwd =
    cwd ??
    focusedEnv?.folderPaths?.[0] ??
    vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ??
    process.cwd();
  mostRecentISLCwd = initialCwd;
  let disposed = false;

  const disposeConnection = onClientConnection({
    postMessage(message: string) {
      if (disposed) {
        return Promise.resolve(false);
      }
      try {
        return panelOrView.webview.postMessage(message) as Promise<boolean>;
      } catch (err) {
        if (disposed) {
          logger.info('Ignoring message to disposed ISL webview');
          return Promise.resolve(false);
        }
        throw err;
      }
    },
    onDidReceiveMessage(handler) {
      return panelOrView.webview.onDidReceiveMessage(m => {
        const isBinary = m instanceof ArrayBuffer;
        handler(m, isBinary);
      });
    },
    cwd: initialCwd,
    onChangeCwd: cwd => {
      mostRecentISLCwd = cwd;
    },
    platform: updatedPlatform,
    appMode: mode,
    logger,
    command: getCLICommand(),
    version: extensionVersion,
    readySignal,
  });

  panelOrView.onDidDispose(() => {
    disposed = true;
    if (isPanel(panelOrView)) {
      logger.info('Disposing ISL panel');
      if (islPanelOrViewResult?.panel === panelOrView) {
        islPanelOrViewResult = undefined;
      }
    } else {
      logger.info('Disposing ISL view');
    }
    disposeConnection();
  });

  return {panel: panelOrView, readySignal};
}

/**
 * Post a message to the ISL webview, if one is currently open.
 * Returns true if the message was sent, false if no webview is open.
 */
export function postMessageToISLWebview(message: ServerToClientMessage): boolean {
  if (islPanelOrViewResult == null) {
    return false;
  }
  islPanelOrViewResult.panel.webview.postMessage(serializeToString(message));
  return true;
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
