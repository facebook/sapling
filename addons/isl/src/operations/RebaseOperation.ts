/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ApplyPreviewsFuncType, PreviewContext} from '../previews';
import type {Hash, Revset} from '../types';

import {CommitPreview} from '../previews';
import {SucceedableRevset} from '../types';
import {Operation} from './Operation';

export class RebaseOperation extends Operation {
  constructor(private source: Hash, private destination: Revset) {
    super();
  }

  static opName = 'Rebase';

  getArgs() {
    return [
      'rebase',
      '-s',
      SucceedableRevset(this.source),
      '-d',
      SucceedableRevset(this.destination),
    ];
  }

  makePreviewApplier(context: PreviewContext): ApplyPreviewsFuncType | undefined {
    const {treeMap} = context;
    const originalSourceNode = treeMap.get(this.source);
    if (originalSourceNode == null) {
      return undefined;
    }
    const newSourceNode = {
      ...originalSourceNode,
      info: {...originalSourceNode.info},
    };
    let parentHash: Hash;

    const func: ApplyPreviewsFuncType = (tree, previewType, childPreviewType) => {
      if (tree.info.hash === this.source) {
        if (tree.info.parents[0] === parentHash) {
          // this is the newly added node
          return {
            info: tree.info,
            children: tree.children,
            previewType: CommitPreview.REBASE_ROOT, // root will show confirmation button
            childPreviewType: CommitPreview.REBASE_DESCENDANT, // children should also show as previews, but don't all need the confirm button
          };
        } else {
          // this is the original source node
          return {
            info: tree.info,
            children: tree.children,
            previewType: CommitPreview.REBASE_OLD,
            childPreviewType: CommitPreview.REBASE_OLD,
          };
        }
      } else if (
        tree.info.hash === this.destination ||
        tree.info.remoteBookmarks.includes(this.destination)
      ) {
        parentHash = tree.info.hash;
        newSourceNode.info.parents = [parentHash];
        // we always want the rebase preview to be the lowest child aka last in list
        return {
          info: tree.info,
          children: [...tree.children, newSourceNode],
        };
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

  makeOptimisticApplier(context: PreviewContext): ApplyPreviewsFuncType | undefined {
    const {treeMap} = context;
    const originalSourceNode = treeMap.get(this.source);
    if (originalSourceNode == null) {
      // once we don't see the source anymore, we don't have optimistic state to apply (that commit has been rebased).
      return undefined;
    }

    const newSourceNode = {
      ...originalSourceNode,
      info: {...originalSourceNode.info},
    };
    let parentHash: Hash;

    const func: ApplyPreviewsFuncType = (tree, previewType, childPreviewType) => {
      if (tree.info.hash === this.source) {
        if (tree.info.parents[0] === parentHash) {
          // this is the newly added node
          return {
            info: tree.info,
            children: tree.children,
            previewType: CommitPreview.REBASE_OPTIMISTIC_ROOT, // root will show spinner
            childPreviewType: CommitPreview.REBASE_OPTIMISTIC_DESCENDANT, // children should also show as previews, but don't all need spinner
          };
        } else {
          // this is the original source node, it's hidden
          return {info: null};
        }
      } else if (
        tree.info.hash === this.destination ||
        tree.info.remoteBookmarks.includes(this.destination)
      ) {
        parentHash = tree.info.hash;
        newSourceNode.info.parents = [parentHash];
        // we always want the rebase preview to be the lowest child aka last in list
        return {info: tree.info, children: [...tree.children, newSourceNode]};
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
