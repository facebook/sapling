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

export class RevertOperation extends Operation {
  static opName = 'Revert';

  constructor(private files: Array<RepoRelativePath>) {
    super();
  }

  getArgs() {
    const args = [
      'revert',
      ...this.files.map(file =>
        // tag file arguments specialy so the remote repo can convert them to the proper cwd-relative format.
        ({
          type: 'repo-relative-file' as const,
          path: file,
        }),
      ),
    ];
    return args;
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    const filesToHide = new Set(this.files);
    if (context.uncommittedChanges.every(change => !filesToHide.has(change.path))) {
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      return changes.filter(change => !filesToHide.has(change.path));
    };
    return func;
  }
}
