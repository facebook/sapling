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
import type {CommandArg, RepoRelativePath, UncommittedChanges} from '../types';

import {Operation} from './Operation';

export class ShelveOperation extends Operation {
  /**
   * @param name the name of the shelved changes. This makes it easier to find the change later.
   * @param filesPathsToCommit if provided, only these file paths will be included in the shelve operation. If undefined, ALL uncommitted changes are included. Paths should be relative to repo root.
   */
  constructor(private name?: string, private filesPathsToCommit?: Array<RepoRelativePath>) {
    super('ShelveOperation');
  }

  static opName = 'Shelve';

  getArgs() {
    const args: Array<CommandArg> = ['shelve', '--unknown'];
    if (this.name) {
      args.push('--name', this.name);
    }
    if (this.filesPathsToCommit) {
      args.push(
        ...this.filesPathsToCommit.map(file =>
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
    const filesToCommit = new Set(this.filesPathsToCommit);
    // optimistic state is over when there's no uncommitted changes that we wanted to shelve left
    if (
      context.uncommittedChanges.length === 0 ||
      (filesToCommit.size > 0 &&
        context.uncommittedChanges.every(change => !filesToCommit.has(change.path)))
    ) {
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      if (this.filesPathsToCommit != null) {
        return changes.filter(change => !filesToCommit.has(change.path));
      } else {
        return [];
      }
    };
    return func;
  }
}

/** Find appropriate ShelveOperation based on selection. */
export function getShelveOperation(
  name: string | undefined,
  selection: PartialSelection,
  allFiles: Array<RepoRelativePath>,
): ShelveOperation {
  if (selection.hasChunkSelection()) {
    throw new Error('Cannot shelve partially selected hunks.');
  } else if (selection.isEverythingSelected(() => allFiles)) {
    return new ShelveOperation(name);
  } else {
    const selectedFiles = allFiles.filter(path => selection.isFullyOrPartiallySelected(path));
    return new ShelveOperation(name, selectedFiles);
  }
}
