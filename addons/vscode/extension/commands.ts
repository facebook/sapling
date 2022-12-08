/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Comparison} from 'shared/Comparison';

import {encodeSaplingDiffUri} from './DiffContentProvider';
import {t} from './i18n';
import {repositoryCache} from 'isl-server/src/RepositoryCache';
import path from 'path';
import {ComparisonType, labelForComparison} from 'shared/Comparison';
import * as vscode from 'vscode';

export function registerCommands(): Array<vscode.Disposable> {
  const disposables: Array<vscode.Disposable> = [];

  const makeDiffHandler = (comparison: Comparison) => {
    return (uriParam?: vscode.Uri) => {
      const uri = uriParam ?? vscode.window.activeTextEditor?.document.uri;
      if (uri == null) {
        vscode.window.showErrorMessage(t('No text document provided to open diff.'));
        return;
      }

      openDiffView(uri, comparison);
    };
  };

  disposables.push(
    vscode.commands.registerCommand(
      'sapling.open-file-diff-uncommitted',
      makeDiffHandler({type: ComparisonType.UncommittedChanges}),
    ),
  );
  disposables.push(
    vscode.commands.registerCommand(
      'sapling.open-file-diff-head',
      makeDiffHandler({type: ComparisonType.HeadChanges}),
    ),
  );
  disposables.push(
    vscode.commands.registerCommand(
      'sapling.open-file-diff-stack',
      makeDiffHandler({type: ComparisonType.StackChanges}),
    ),
  );

  return disposables;
}

function openDiffView(uri: vscode.Uri, comparison: Comparison): void {
  const {fsPath} = uri;
  const repo = repositoryCache.cachedRepositoryForPath(fsPath);
  if (repo == null) {
    vscode.window.showErrorMessage(t(`No repository found for file ${fsPath}`));
    return;
  }
  const left = encodeSaplingDiffUri(uri, comparison);
  const right = uri;
  const title = `${path.basename(fsPath)} (${t(labelForComparison(comparison))})`;

  vscode.commands.executeCommand('vscode.diff', left, right, title);
}
