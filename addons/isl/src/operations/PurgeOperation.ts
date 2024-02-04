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
import type {CommandArg, RepoRelativePath, UncommittedChanges} from '../types';

import {Operation} from './Operation';

/**
 * This deletes untracked files from disk. Often used in conjunction with "Discard" aka `goto --clean .`
 * If an array of files is provided, only purge those files.
 */
export class PurgeOperation extends Operation {
  static opName = 'Purge';

  constructor(private files: Array<RepoRelativePath> = []) {
    super('PurgeOperation');
  }

  getArgs() {
    const args: Array<CommandArg> = ['purge', '--files'];
    args.push(
      ...this.files.map(file => ({
        type: 'repo-relative-file' as const,
        path: file,
      })),
    );
    return args;
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    const filesToHide = new Set(this.files);
    if (
      context.uncommittedChanges.length === 0 ||
      // no untracked files should be left
      context.uncommittedChanges.every(change => !filesToHide.has(change.path))
    ) {
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      // remove all untracked files
      return changes.filter(change => !filesToHide.has(change.path));
    };
    return func;
  }
}
