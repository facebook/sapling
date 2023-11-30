/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitTree} from '../getCommitTree';
import type {ApplyPreviewsFuncType, PreviewContext} from '../previews';
import type {ExactRevset, Hash, SucceedableRevset} from '../types';

import {latestSuccessor} from '../SuccessionTracker';
import {CommitPreview} from '../previews';
import {Operation} from './Operation';

export class BulkRebaseOperation extends Operation {
  constructor(
    private sources: Array<SucceedableRevset>,
    private destination: ExactRevset | SucceedableRevset,
  ) {
    super('BulkRebaseOperation');
  }

  static opName = 'Bulk rebase commits';

  getArgs() {
    return [
      'rebase',
      ...this.sources.map(source => ['--rev', source]).flat(),
      '-d',
      this.destination,
    ];
  }

  makeOptimisticApplier(context: PreviewContext): ApplyPreviewsFuncType | undefined {
    const {treeMap} = context;
    const stackBasesToRebase = this.sources
      .map(revset => treeMap.get(revset.revset))
      .filter((tree): tree is CommitTree => {
        if (tree == null) {
          return false;
        }
        const parent = treeMap.get(tree.info.parents[0]);
        // Only commits which are the base of a stack that aren't already on the destination
        return (
          parent != null &&
          parent.info.phase === 'public' &&
          parent.info.hash !== latestSuccessor(context, this.destination) &&
          !parent.info.remoteBookmarks.includes(this.destination.revset)
        );
      })
      .map(tree => {
        // make a copy of the tree
        return {
          ...tree,
          info: {
            ...tree.info,
          },
        };
      });

    if (stackBasesToRebase.length === 0) {
      // once there's no stacks on public commits that aren't the destination, then we're done
      return undefined;
    }

    let parentHash: Hash;

    const func: ApplyPreviewsFuncType = (tree, previewType, childPreviewType) => {
      if (stackBasesToRebase.find(toRebase => toRebase.info.hash === tree.info.hash)) {
        if (tree.info.parents[0] === parentHash) {
          // this is a newly added node
          return {
            info: tree.info,
            children: tree.children,
            previewType: CommitPreview.REBASE_OPTIMISTIC_ROOT, // root will show spinner
            childPreviewType: CommitPreview.REBASE_OPTIMISTIC_DESCENDANT, // children should also show as previews, but don't all need spinner
          };
        } else {
          // this is an original source node, it's hidden
          return {info: null};
        }
      } else if (
        tree.info.hash === latestSuccessor(context, this.destination) ||
        tree.info.remoteBookmarks.includes(this.destination.revset)
      ) {
        parentHash = tree.info.hash;

        stackBasesToRebase.forEach(toRebase => {
          toRebase.info.parents = [parentHash];
        });
        // we always want the rebase previews to be the lowest child aka last in list
        return {info: tree.info, children: [...tree.children, ...stackBasesToRebase]};
      } else {
        return {
          info: tree.info,
          children: tree.children,
          previewType,
          // inherit previews so entire subtree is previewed
          childPreviewType,
        };
      }
    };
    return func;
  }
}
