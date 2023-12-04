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
import type {CommitInfo, UncommittedChanges} from '../types';

import {Operation} from './Operation';

export class UncommitOperation extends Operation {
  /**
   * @param originalHeadCommit the current head commit, needed to track when optimistic state is resolved and get the list of files that will be uncommitted
   */
  constructor(private originalHeadCommit: CommitInfo) {
    super('UncommitOperation');
  }

  static opName = 'Uncommit';

  getArgs() {
    const args = ['uncommit'];
    return args;
  }

  optimisticDag(dag: Dag): Dag {
    const {hash, parents} = this.originalHeadCommit;
    const p1 = parents.at(0);
    // If `hash` disappears and `p1` still exists, then uncommit is completed.
    // We assume uncommit is always run from the stack top.
    if (dag.get(hash) == null || p1 == null || dag.get(p1) == null) {
      return dag;
    }
    // Hide `hash` and set `isHead` on `p1`.
    return dag.replaceWith([p1, hash], (h, c) => {
      if (h === hash) {
        return undefined;
      } else {
        return c && {...c, isHead: true};
      }
    });
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    const uncommittedChangesAfterUncommit = this.originalHeadCommit.filesSample;
    const preexistingChanges = new Set(context.uncommittedChanges.map(change => change.path));

    if (uncommittedChangesAfterUncommit.every(file => preexistingChanges.has(file.path))) {
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
      const newChanges = uncommittedChangesAfterUncommit.filter(
        file => !preexistingChanges.has(file.path),
      );
      return [...changes, ...newChanges];
    };
    return func;
  }
}
