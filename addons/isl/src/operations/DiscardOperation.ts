/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PartialSelection} from '../partialSelection';
import type {
  ApplyUncommittedChangesPreviewsFuncType,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {RepoRelativePath, UncommittedChanges} from '../types';
import type {ImportStack} from 'shared/types/stack';

import {t} from '../i18n';
import {Operation} from './Operation';

/**
 * "Discard" is not an actual command, but the effect of removing all uncommitted changes is accomplished by `goto --clean .`
 * This leaves behind untracked files, which may be separately removed by `purge --files`.
 */
export class DiscardOperation extends Operation {
  static opName = 'Discard';

  constructor() {
    super('DiscardOperation');
  }

  getArgs() {
    const args = ['goto', '--clean', '.'];
    return args;
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    const trackedChangeTypes = ['M', 'A', 'R', '!'];
    if (
      context.uncommittedChanges.length === 0 ||
      // some files may become untracked after clean goto
      context.uncommittedChanges.every(change => !trackedChangeTypes.includes(change.status))
    ) {
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      // clean goto leaves behind untracked files
      return changes.filter(change => !trackedChangeTypes.includes(change.status));
    };
    return func;
  }
}

/**
 * Removes selected uncommitted changes to make them disappare from `status` or `diff`.
 * - Delete selected `A` or `?` files, so they disappear from `status`
 * - Restore selected `R` or `!` files, so they disappear from `status`.
 * - Revert selected `M` files, so they disappear from status.
 * - For partially selected files, their content will be reverted by dropping
 *   the selected changes (line insertions or deletions), similar to `revert -i`.
 *   The selected changes will disappear from `diff` output.
 *
 * This might replace DiscardOperation in the future. For now they might still be different,
 * since this operation only writes the working copy without changing the dirstate.
 */
export class PartialDiscardOperation extends Operation {
  static opName = 'Discard';

  constructor(private selection: PartialSelection, private allFiles: Array<RepoRelativePath>) {
    super('PartialDiscardOperation');
  }

  getArgs() {
    return ['debugimportstack'];
  }

  getStdin(): string | undefined {
    const inverse = true;
    const files = this.selection.calculateImportStackFiles(this.allFiles, inverse);
    const importStack: ImportStack = [['write', files]];
    return JSON.stringify(importStack);
  }

  getDescriptionForDisplay() {
    return {
      description: t('Discarding selected changes'),
      tooltip: t(
        'This operation does not have a traditional command line equivalent. \n' +
          'You can use `revert -i`, `goto --clean`, `purge` for similar effects.',
      ),
    };
  }

  makeOptimisticUncommittedChangesApplier?(
    _context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      return changes.filter(change => !this.selection.isFullySelected(change.path));
    };
    return func;
  }
}
