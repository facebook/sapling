/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ApplyPreviewsFuncType, DagWithPreview, PreviewContext} from '../previews';
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

  optimisticDag(dag: DagWithPreview): DagWithPreview {
    const hash = this.source.revset;
    const toHide = dag.descendants(hash);
    // If the head is being hidden, we need to move the head to the parent.
    const newHead = [];
    if (toHide.toHashes().some(h => dag.get(h)?.isHead == true)) {
      const parent = dag.get(hash)?.parents?.at(0);
      if (parent && dag.has(parent)) {
        newHead.push(parent);
      }
    }
    return dag.remove(toHide).replaceWith(newHead, (_h, c) => {
      return c && {...c, isHead: true, previewType: CommitPreview.GOTO_DESTINATION};
    });
  }
}
