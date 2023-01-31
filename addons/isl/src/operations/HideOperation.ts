/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ApplyPreviewsFuncType, PreviewContext} from '../previews';
import type {Hash} from '../types';

import {CommitPreview} from '../previews';
import {SucceedableRevset} from '../types';
import {Operation} from './Operation';

export class HideOperation extends Operation {
  constructor(private source: Hash) {
    super();
  }

  static opName = 'Hide';

  getArgs() {
    return ['hide', '-r', SucceedableRevset(this.source)];
  }

  makePreviewApplier(_context: PreviewContext): ApplyPreviewsFuncType | undefined {
    console.log('Creating applier');
    const func: ApplyPreviewsFuncType = (tree, previewType, childPreviewType) => {
      if (tree.info.hash === this.source) {
        console.log(`Applier created to ${tree.info.hash}`);
        return {
          info: tree.info,
          children: tree.children,
          previewType: CommitPreview.HIDDEN_ROOT,
          childPreviewType: CommitPreview.HIDDEN_DESCENDANT,
        };
      }
      return {
        info: tree.info,
        children: tree.children,
        previewType,
        childPreviewType,
      };
    };
    return func;
  }

  makeOptimisticApplier(context: PreviewContext): ApplyPreviewsFuncType | undefined {
    const {treeMap} = context;
    const originalSourceNode = treeMap.get(this.source);
    if (originalSourceNode == null) {
      return undefined;
    }

    const func: ApplyPreviewsFuncType = (tree, previewType, childPreviewType) => {
      if (tree.info.hash === this.source) {
        return {
          info: null,
        };
      }
      return {
        info: tree.info,
        children: tree.children,
        previewType,
        childPreviewType,
      };
    };
    return func;
  }
}
