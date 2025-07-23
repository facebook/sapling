/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  ApplyUncommittedChangesPreviewsFuncType,
  Dag,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {ChangedFile, CommitInfo, UncommittedChanges} from '../types';

import {Operation} from './Operation';

export class UncommitOperation extends Operation {
  /**
   * @param originalDotCommit the current dot commit, needed to track when optimistic state is resolved
   * @param changedFiles the files that are in the commit to be uncommitted. Must be fetched before running, as the CommitInfo object itself does not have file statuses.
   */
  constructor(
    private originalDotCommit: CommitInfo,
    private changedFiles: Array<ChangedFile>,
  ) {
    super('UncommitOperation');
  }

  static opName = 'Uncommit';

  getArgs() {
    const args = ['uncommit'];
    return args;
  }

  optimisticDag(dag: Dag): Dag {
    const {hash, parents} = this.originalDotCommit;
    const p1 = parents.at(0);
    const commitHasChildren = (dag.children(hash)?.size ?? 0) > 0;
    if (
      p1 == null || commitHasChildren
        ? // If the commit has children, then we know the uncommit is done when it's no longer the dot commit
          dag.get(hash)?.isDot !== true
        : // If the commit does not have children, if `hash` disappears and `p1` still exists, then uncommit is completed.
          dag.get(hash) == null || dag.get(p1) == null
    ) {
      return dag;
    }
    return commitHasChildren
      ? // Set `isDot` on `p1` and not `hash`
        dag.replaceWith([p1 as string, hash], (h, c) => {
          return c?.set('isDot', h === p1);
        })
      : // Hide `hash` and set `isDot` on `p1`.
        dag.replaceWith([p1 as string, hash], (h, c) => {
          if (h === hash) {
            return undefined;
          } else {
            return c?.set('isDot', true);
          }
        });
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    const preexistingChanges = new Set(context.uncommittedChanges.map(change => change.path));

    if (this.changedFiles.every(file => preexistingChanges.has(file.path))) {
      // once every file to uncommit appears in the output, the uncommit has reflected in the latest fetch.
      // TODO: we'll eventually limit how many uncommitted changes we pull in. When this happens, it's
      // possible the list of files won't include any of the changes being uncommitted (though this would be rare).
      // We should probably return undefined if the number of uncommitted changes >= max fetched.
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      // You could have uncommitted changes before uncommitting, so we need to include
      // files from the commit AND the existing uncommitted changes.
      // But it's also possible to have changed a file changed by the commit, so we need to de-dupe.
      const newChanges = this.changedFiles.filter(file => !preexistingChanges.has(file.path));
      return [...changes, ...newChanges];
    };
    return func;
  }
}
