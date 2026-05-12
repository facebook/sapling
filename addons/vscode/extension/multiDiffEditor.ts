/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFile} from 'isl/src/types';
import * as path from 'path';
import type {Comparison} from 'shared/Comparison';
import {
  ComparisonType,
  beforeRevsetForComparison,
  comparisonStringKey,
  currRevsetForComparison,
  labelForComparison,
} from 'shared/Comparison';
import lazyInit from 'shared/lazyInit';
import * as vscode from 'vscode';
import {encodeDeletedFileUri} from './DeletedFileContentProvider';
import {encodeSaplingDiffUri} from './DiffContentProvider';
import {t} from './i18n';

const OPEN_MULTI_DIFF_EDITOR_COMMAND = '_workbench.openMultiDiffEditor';

export const hasMultiDiffEditorSupport = lazyInit(async () => {
  try {
    const commands = await vscode.commands.getCommands();
    return commands.includes(OPEN_MULTI_DIFF_EDITOR_COMMAND);
  } catch {
    return false;
  }
});

/**
 * Opens VS Code's native multi-diff editor for a given comparison.
 * This constructs the appropriate URIs for each changed file and calls
 * the internal `_workbench.openMultiDiffEditor` command.
 *
 * @param repoRoot - The absolute path to the repository root
 * @param comparison - The comparison type (uncommitted, head, stack, or committed)
 * @param files - Array of changed files with path and status
 */
export async function openMultiDiffEditor(
  repoRoot: string,
  comparison: Comparison,
  files: Array<ChangedFile>,
): Promise<void> {
  if (files.length === 0) {
    vscode.window.showInformationMessage(t('No changed files to display'));
    return;
  }

  if (!(await hasMultiDiffEditorSupport())) {
    throw new Error('Multi-diff editor not supported in this version of VS Code');
  }

  const beforeRevset = beforeRevsetForComparison(comparison);
  const currentRevset = currRevsetForComparison(comparison);

  // For uncommitted/head/stack changes, the right side is the working copy (editable)
  // For committed changes, both sides are read-only from specific revisions
  const isWorkingCopyComparison =
    comparison.type === ComparisonType.UncommittedChanges ||
    comparison.type === ComparisonType.HeadChanges ||
    comparison.type === ComparisonType.StackChanges;

  const resources = files.map(file => {
    const fileUri = vscode.Uri.file(path.join(repoRoot, file.path));

    // Determine if file is newly added or deleted
    const isAdded = file.status === 'A' || file.status === '?';
    const isRemoved = file.status === 'R' || file.status === '!';

    // Build original URI (left side of diff)
    // For added files, there is no original - use undefined
    const originalUri = isAdded ? undefined : encodeSaplingDiffUri(fileUri, beforeRevset);

    // Build modified URI (right side of diff)
    // For removed files, use the deleted file provider (empty content)
    // For working copy comparisons, use the raw file:// URI (editable)
    // For committed comparisons, use encoded URI at the commit's revision
    let modifiedUri: vscode.Uri | undefined;
    if (isRemoved) {
      modifiedUri = encodeDeletedFileUri(fileUri);
    } else if (isWorkingCopyComparison) {
      modifiedUri = fileUri; // Raw file:// URI allows editing
    } else {
      modifiedUri = encodeSaplingDiffUri(fileUri, currentRevset);
    }

    return {
      originalUri,
      modifiedUri,
    };
  });

  const title = t(labelForComparison(comparison));

  // Include repoRoot so each repo gets its own multi-diff tab, and use
  // comparisonStringKey so different comparisons of the same type don't collide.
  const multiDiffSourceUri = vscode.Uri.from({
    scheme: 'sapling-comparison',
    path: `${repoRoot}/${comparisonStringKey(comparison)}`,
  });

  await vscode.commands.executeCommand(OPEN_MULTI_DIFF_EDITOR_COMMAND, {
    title,
    multiDiffSourceUri,
    resources,
  });
}
