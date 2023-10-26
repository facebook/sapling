/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  ApplyUncommittedChangesPreviewsFuncType,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {
  CommandArg,
  ExactRevset,
  RepoRelativePath,
  SucceedableRevset,
  UncommittedChanges,
} from '../types';

import {Operation} from './Operation';

export class RevertOperation extends Operation {
  static opName = 'Revert';

  constructor(
    private files: Array<RepoRelativePath>,
    private revset?: SucceedableRevset | ExactRevset,
  ) {
    super('RevertOperation');
  }

  getArgs() {
    const args: Array<CommandArg> = ['revert'];
    if (this.revset != null) {
      args.push('--rev', this.revset);
    }
    args.push(
      ...this.files.map(file =>
        // tag file arguments specialy so the remote repo can convert them to the proper cwd-relative format.
        ({
          type: 'repo-relative-file' as const,
          path: file,
        }),
      ),
    );
    return args;
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    if (this.revset == null) {
      const filesToHide = new Set(this.files);
      if (context.uncommittedChanges.every(change => !filesToHide.has(change.path))) {
        return undefined;
      }

      const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
        return changes.filter(change => !filesToHide.has(change.path));
      };
      return func;
    } else {
      // If reverting back to a specific commit, the file will probably become 'M', not disappear.
      // Note: this is just a guess, in reality the file could do any number of things.

      const filesToMarkChanged = new Set(this.files);
      if (context.uncommittedChanges.find(change => filesToMarkChanged.has(change.path)) != null) {
        return undefined;
      }
      const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
        const existingChanges = new Set(changes.map(change => change.path));
        const revertedChangesToInsert = this.files.filter(file => !existingChanges.has(file));
        return [
          ...changes.map(change =>
            filesToMarkChanged.has(change.path) ? {...change, status: 'M' as const} : change,
          ),
          ...revertedChangesToInsert.map(path => ({path, status: 'M' as const})),
        ];
      };
      return func;
    }
  }
}
