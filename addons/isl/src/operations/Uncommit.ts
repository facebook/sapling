/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  ApplyPreviewsFuncType,
  ApplyUncommittedChangesPreviewsFuncType,
  PreviewContext,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {CommitInfo, UncommittedChanges} from '../types';

import {Operation} from './Operation';

export class UncommitOperation extends Operation {
  /**
   * @param originalHeadCommit the current head commit, needed to track when optimistic state is resolved and get the list of files that will be uncommitted
   */
  constructor(private originalHeadCommit: CommitInfo) {
    super();
  }

  static opName = 'Uncommit';

  getArgs() {
    const args = ['uncommit'];
    return args;
  }

  makeOptimisticApplier(context: PreviewContext): ApplyPreviewsFuncType | undefined {
    const headCommitHash = context.headCommit?.hash;
    if (headCommitHash !== this.originalHeadCommit.hash) {
      // head hash has changed -> uncommit was successful
      return undefined;
    }

    const func: ApplyPreviewsFuncType = (tree, previewType) => {
      if (tree.info.hash === this.originalHeadCommit.hash) {
        // uncommit may not be run on a commit with children
        // thus, we can just hide the head commit from the tree
        return {
          info: null,
        };
      } else if (this.originalHeadCommit.parents[0] === tree.info.hash) {
        // head will move to parent commit after uncommitting
        return {
          info: {...tree.info, isHead: true},
          children: tree.children, // the parent may have other children than the one being uncommitted
        };
      }

      return {info: tree.info, children: tree.children, previewType};
    };
    return func;
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
