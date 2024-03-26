/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from 'isl-server/src/Repository';
import type {RepositoryContext} from 'isl-server/src/serverTypes';
import type {Operation} from 'isl/src/operations/Operation';
import type {RepoRelativePath} from 'isl/src/types';
import type {Comparison} from 'shared/Comparison';

import {encodeDeletedFileUri} from './DeletedFileContentProvider';
import {encodeSaplingDiffUri} from './DiffContentProvider';
import {t} from './i18n';
import fs from 'fs';
import {repoRelativePathForAbsolutePath} from 'isl-server/src/Repository';
import {repositoryCache} from 'isl-server/src/RepositoryCache';
import {findPublicAncestor} from 'isl-server/src/utils';
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

  ['sapling.open-remote-file-link']: commandWithUriOrResourceState(
    (repo: Repository, uri, path: RepoRelativePath) => openRemoteFileLink(repo, uri, path),
  ),
  ['sapling.copy-remote-file-link']: commandWithUriOrResourceState(
    (repo: Repository, uri, path: RepoRelativePath) => openRemoteFileLink(repo, uri, path, true),
  ),

  ['sapling.revert-file']: commandWithUriOrResourceState(async function (
    this: RepositoryContext,
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
    return runOperation(this, repo, new RevertOperation([path]));
  }),
};

type surveyMetaData = {
  diffId: string | undefined;
};

/** Type definitions for built-in or third-party VS Code commands we want to execute programatically. */
type ExternalVSCodeCommands = {
  'vscode.diff': (left: vscode.Uri, right: vscode.Uri, title: string) => Thenable<unknown>;
  'workbench.action.closeSidebar': () => Thenable<void>;
  'fb.survey.initStateUIByNamespace': (
    surveyID: string,
    namespace: string,
    metadata: surveyMetaData,
  ) => Thenable<void>;
  'sapling.open-isl': () => Thenable<void>;
  'sapling.close-isl': () => Thenable<void>;
  'sapling.isl.focus': () => Thenable<void>;
  setContext: (key: string, value: unknown) => Thenable<void>;
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
  // In tests 'vscode.commands' is not defined.
  return vscode.commands?.executeCommand(id, ...args) as ReturnType<VSCodeCommand[K]>;
}

const runOperation = (
  ctx: RepositoryContext,
  repo: Repository,
  operation: Operation,
): undefined => {
  repo.runOrQueueOperation(
    ctx,
    {
      args: operation.getArgs(),
      id: operation.id,
      runner: operation.runner,
      trackEventName: operation.trackEventName,
    },
    () => undefined, // TODO: Send this progress info to any existing ISL webview if there is one
  );
  return undefined;
};

export function registerCommands(ctx: RepositoryContext): Array<vscode.Disposable> {
  const disposables: Array<vscode.Disposable> = Object.entries(vscodeCommands).map(
    ([id, handler]) =>
      vscode.commands.registerCommand(id, (...args: Parameters<typeof handler>) =>
        ctx.tracker.operation(
          'RunVSCodeCommand',
          'VSCodeCommandError',
          {extras: {command: id}},
          () => {
            return (handler as (...args: Array<unknown>) => unknown).apply(ctx, args);
          },
        ),
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

function openRemoteFileLink(
  repo: Repository,
  uri: vscode.Uri,
  path: RepoRelativePath,
  copyToClipboard = false,
): void {
  {
    if (!repo.codeReviewProvider?.getRemoteFileURL) {
      vscode.window.showErrorMessage(
        t(`Remote link unsupported for this code review provider ($provider)`).replace(
          '$provider',
          repo.codeReviewProvider?.getSummaryName() ?? t('none'),
        ),
      );
      return;
    }

    // Grab the selection if the command is for the active file (may not be true if triggered via file explorer)
    const selection =
      vscode.window.activeTextEditor?.document.uri.fsPath === uri.fsPath
        ? vscode.window.activeTextEditor?.selection
        : null;

    const commits = repo.getSmartlogCommits()?.commits.value;
    const head = repo.getHeadCommit();
    if (!commits || !head) {
      vscode.window.showErrorMessage(t(`No commits loaded in this repository yet`));
      return;
    }
    const publicCommit = findPublicAncestor(commits, head);
    const url = repo.codeReviewProvider.getRemoteFileURL(
      path,
      publicCommit?.hash ?? null,
      selection ? {line: selection.start.line, char: selection.start.character} : undefined,
      selection ? {line: selection.end.line, char: selection.end.character} : undefined,
    );

    if (copyToClipboard) {
      vscode.env.clipboard.writeText(url);
    } else {
      vscode.env.openExternal(vscode.Uri.parse(url));
    }
  }
}

/**
 * Wrap a command implementation so it can be called with any of:
 * - current active file Uri for use from the command palette
 * - a vscode Uri for programmatic invocations
 * - a SourceControlResourceState for use from the VS Code SCM sidebar API
 */
function commandWithUriOrResourceState(
  handler: (
    repo: Repository,
    uri: vscode.Uri,
    path: RepoRelativePath,
  ) => unknown | Thenable<unknown>,
) {
  return function (
    this: RepositoryContext,
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
