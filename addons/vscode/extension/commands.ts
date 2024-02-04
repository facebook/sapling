/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from 'isl-server/src/Repository';
import type {ServerSideTracker} from 'isl-server/src/analytics/serverSideTracker';
import type {Operation} from 'isl/src/operations/Operation';
import type {RepoRelativePath} from 'isl/src/types';
import type {Comparison} from 'shared/Comparison';

import {encodeDeletedFileUri} from './DeletedFileContentProvider';
import {encodeSaplingDiffUri} from './DiffContentProvider';
import {t} from './i18n';
import fs from 'fs';
import {repoRelativePathForAbsolutePath} from 'isl-server/src/Repository';
import {repositoryCache} from 'isl-server/src/RepositoryCache';
import {RevertOperation} from 'isl/src/operations/RevertOperation';
import path from 'path';
import {ComparisonType, labelForComparison} from 'shared/Comparison';
import * as vscode from 'vscode';

/**
 * VS Code Commands registered by the Sapling extension.
 */
export const vscodeCommands = {
  ['sapling.open-file-diff-uncommitted']: commandWithUriOrResourceState((_, uri: vscode.Uri) =>
    openDiffView(uri, {type: ComparisonType.UncommittedChanges}),
  ),
  ['sapling.open-file-diff-head']: commandWithUriOrResourceState((_, uri: vscode.Uri) =>
    openDiffView(uri, {type: ComparisonType.HeadChanges}),
  ),
  ['sapling.open-file-diff-stack']: commandWithUriOrResourceState((_, uri: vscode.Uri) =>
    openDiffView(uri, {type: ComparisonType.StackChanges}),
  ),
  ['sapling.open-file-diff']: (uri: vscode.Uri, comparison: Comparison) =>
    openDiffView(uri, comparison),

  ['sapling.revert-file']: commandWithUriOrResourceState(async function (
    this: Context,
    repo: Repository,
    _,
    path: RepoRelativePath,
  ) {
    const choice = await vscode.window.showWarningMessage(
      'Are you sure you want to revert this file?',
      'Cancel',
      'Revert',
    );
    if (choice !== 'Revert') {
      return;
    }
    return runOperation(repo, new RevertOperation([path]), this.tracker);
  }),
};

/** Type definitions for built-in or third-party VS Code commands we want to execute programatically. */
type ExternalVSCodeCommands = {
  'vscode.diff': (left: vscode.Uri, right: vscode.Uri, title: string) => Thenable<unknown>;
  'workbench.action.closeSidebar': () => Thenable<void>;
  'fb.survey.initStateUIByNamespace': (surveyID: string) => Thenable<void>;
  'sapling.open-isl': () => Thenable<void>;
  'sapling.close-isl': () => Thenable<void>;
  'sapling.isl.focus': () => Thenable<void>;
  'fb-hg.open-or-focus-interactive-smartlog': (
    _: unknown,
    __?: unknown,
    forceNoSapling?: boolean,
  ) => Thenable<void>;
};

export type VSCodeCommand = typeof vscodeCommands & ExternalVSCodeCommands;

/**
 * Type-safe programmatic execution of VS Code commands (via `vscode.commands.executeCommand`).
 * Sapling-provided commands are defiend in vscodeCommands.
 * Built-in or third-party commands may also be typed through this function,
 * just define them in ExternalVSCodeCommands.
 */
export function executeVSCodeCommand<K extends keyof VSCodeCommand>(
  id: K,
  ...args: Parameters<VSCodeCommand[K]>
): ReturnType<VSCodeCommand[K]> {
  return vscode.commands.executeCommand(id, ...args) as ReturnType<VSCodeCommand[K]>;
}

type Context = {
  tracker: ServerSideTracker;
};

const runOperation = (
  repo: Repository,
  operation: Operation,
  tracker: ServerSideTracker,
): undefined => {
  repo.runOrQueueOperation(
    {
      args: operation.getArgs(),
      id: operation.id,
      runner: operation.runner,
      trackEventName: operation.trackEventName,
    },
    () => undefined, // TODO: Send this progress info to any existing ISL webview if there is one
    tracker,
    repo.info.repoRoot,
  );
  return undefined;
};

export function registerCommands(tracker: ServerSideTracker): Array<vscode.Disposable> {
  const context: Context = {
    tracker,
  };

  const disposables: Array<vscode.Disposable> = Object.entries(vscodeCommands).map(
    ([id, handler]) =>
      vscode.commands.registerCommand(id, (...args: Parameters<typeof handler>) =>
        tracker.operation('RunVSCodeCommand', 'VSCodeCommandError', {extras: {command: id}}, () => {
          return (handler as (...args: Array<unknown>) => unknown).apply(context, args);
        }),
      ),
  );
  return disposables;
}

function fileExists(uri: vscode.Uri): Promise<boolean> {
  return fs.promises
    .access(uri.fsPath)
    .then(() => true)
    .catch(() => false);
}

async function openDiffView(uri: vscode.Uri, comparison: Comparison): Promise<unknown> {
  const {fsPath} = uri;
  const title = `${path.basename(fsPath)} (${t(labelForComparison(comparison))})`;
  const uriForComparison = encodeSaplingDiffUri(uri, comparison);
  if (comparison.type !== ComparisonType.Committed) {
    return executeVSCodeCommand(
      'vscode.diff',
      uriForComparison,
      (await fileExists(uri)) ? uri : encodeDeletedFileUri(uri),
      title,
    );
  }
  const uriForComparisonParent = encodeSaplingDiffUri(uri, {
    type: ComparisonType.Committed,
    hash: `${comparison.hash}^`,
  });
  return executeVSCodeCommand('vscode.diff', uriForComparisonParent, uriForComparison, title);
}

/**
 * Wrap a command implementation so it can be called with any of:
 * - current active file Uri for use from the command palette
 * - a vscode Uri for programmatic invocations
 * - a SourceControlResourceState for use from the VS Code SCM sidebar API
 */
function commandWithUriOrResourceState<Ctx>(
  handler: (
    repo: Repository,
    uri: vscode.Uri,
    path: RepoRelativePath,
  ) => undefined | Thenable<unknown>,
) {
  return function (
    this: Ctx,
    uriOrResource: vscode.Uri | vscode.SourceControlResourceState | undefined,
  ) {
    const uri =
      uriOrResource == null
        ? vscode.window.activeTextEditor?.document.uri
        : uriOrResource instanceof vscode.Uri
        ? uriOrResource
        : uriOrResource.resourceUri;
    if (uri == null) {
      vscode.window.showErrorMessage(t(`No active file found`));
      return;
    }

    const {fsPath} = uri;
    const repo = repositoryCache.cachedRepositoryForPath(fsPath);
    if (repo == null) {
      vscode.window.showErrorMessage(t(`No repository found for file ${fsPath}`));
      return;
    }

    const repoRelativePath = repoRelativePathForAbsolutePath(uri.fsPath, repo);
    return handler.apply(this, [repo, uri, repoRelativePath]);
  };
}
