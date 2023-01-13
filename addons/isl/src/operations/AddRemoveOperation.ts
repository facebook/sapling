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
import type {RepoRelativePath, UncommittedChanges} from '../types';

import {Operation} from './Operation';

/**
 * `sl addremove` adds all untracked files, and forgets all missing files.
 * If filepaths are passed, only those file paths will be used, like doing a bulk `sl add` or `sl forget`.
 * If filepaths is empty array, all untracked/missing files will be affected.
 */
export class AddRemoveOperation extends Operation {
  constructor(private paths: Array<RepoRelativePath>) {
    super();
  }

  static opName = 'AddRemove';

  getArgs() {
    return [
      'addremove',
      ...this.paths.map(path => ({
        type: 'repo-relative-file' as const,
        path,
      })),
    ];
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    const allFiles = this.paths.length === 0;
    if (
      context.uncommittedChanges.every(
        change =>
          (allFiles || this.paths.includes(change.path)) &&
          change.status !== '?' &&
          change.status !== '!',
      )
    ) {
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      return changes.map(change =>
        allFiles || this.paths.includes(change.path)
          ? {
              path: change.path,
              status: change.status === '?' ? 'A' : change.status === '!' ? 'R' : change.status,
            }
          : change,
      );
    };
    return func;
  }
}
