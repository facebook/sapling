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

export class AddOperation extends Operation {
  constructor(private filePath: RepoRelativePath) {
    super();
  }

  static opName = 'Add';

  getArgs() {
    return [
      'add',
      {
        type: 'repo-relative-file' as const,
        path: this.filePath,
      },
    ];
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    if (
      context.uncommittedChanges.some(
        change => change.path === this.filePath && change.status !== '?',
      )
    ) {
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      return changes.map(change =>
        change.path === this.filePath ? {path: change.path, status: 'A'} : change,
      );
    };
    return func;
  }
}
