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
 * "Discard" is not an actual command, but the effect of removing all uncommitted changes is accomplished by `goto --clean .`
 * This leaves behind untracked files, which may be separately removed by `purge --files`.
 */
export class DiscardOperation extends Operation {
  static opName = 'Discard';

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
