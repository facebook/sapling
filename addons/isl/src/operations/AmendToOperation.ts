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

export class AmendToOperation extends Operation {
  /**
   * @param filePathsToAmend if provided, only these file paths will be included in the amend operation. If undefined, ALL uncommitted changes are included. Paths should be relative to repo root.
   * @param message if provided, update commit description to use this title & description
   */
  constructor(
    private commit: SucceedableRevset | ExactRevset,
    private filePathsToAmend?: Array<RepoRelativePath>,
  ) {
    super('AmendToOperation');
  }

  static opName = 'AmendTo';

  getArgs() {
    const args: Array<CommandArg> = ['amend', '--to', this.commit];
    if (this.filePathsToAmend) {
      args.push(
        ...this.filePathsToAmend.map(file =>
          // tag file arguments specialy so the remote repo can convert them to the proper cwd-relative format.
          ({
            type: 'repo-relative-file' as const,
            path: file,
          }),
        ),
      );
    }
    return args;
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    const filesToAmend = new Set(this.filePathsToAmend);
    if (
      context.uncommittedChanges.length === 0 ||
      (filesToAmend.size > 0 &&
        context.uncommittedChanges.every(change => !filesToAmend.has(change.path)))
    ) {
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      if (this.filePathsToAmend != null) {
        return changes.filter(change => !filesToAmend.has(change.path));
      } else {
        return [];
      }
    };
    return func;
  }
}
