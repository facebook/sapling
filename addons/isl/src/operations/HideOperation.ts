/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ApplyPreviewsFuncType, PreviewContext} from '../previews';
import type {ExactRevset, SucceedableRevset} from '../types';

import {CommitPreview} from '../previews';
import {Operation} from './Operation';

export class HideOperation extends Operation {
  constructor(private source: ExactRevset | SucceedableRevset) {
    super('HideOperation');
  }

  static opName = 'Hide';

  getArgs() {
    return ['hide', '--rev', this.source];
  }

  makePreviewApplier(_context: PreviewContext): ApplyPreviewsFuncType | undefined {
    const func: ApplyPreviewsFuncType = (tree, previewType) => {
      if (tree.info.hash === this.source.revset) {
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
        childPreviewType: previewType,
      };
    };
    return func;
  }

  makeOptimisticApplier(context: PreviewContext): ApplyPreviewsFuncType | undefined {
    const {treeMap} = context;
    const originalSourceNode = treeMap.get(this.source.revset);
    if (originalSourceNode == null) {
      return undefined;
    }

    const func: ApplyPreviewsFuncType = (tree, previewType, childPreviewType) => {
      if (tree.info.hash === this.source.revset) {
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
