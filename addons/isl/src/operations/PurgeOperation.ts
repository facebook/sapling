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
import type {UncommittedChanges} from '../types';

import {Operation} from './Operation';

/**
 * This deletes untracked files from disk. Often used in conjunction with "Discard" aka `goto --clean .`
 */
export class PurgeOperation extends Operation {
  static opName = 'Purge';

  getArgs() {
    const args = ['purge', '--files'];
    return args;
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    const untrackedChangeTypes = ['?'];
    if (
      context.uncommittedChanges.length === 0 ||
      // no untracked files should be left
      context.uncommittedChanges.every(change => !untrackedChangeTypes.includes(change.status))
    ) {
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      // remove all untracked files
      return changes.filter(change => !untrackedChangeTypes.includes(change.status));
    };
    return func;
  }
}
