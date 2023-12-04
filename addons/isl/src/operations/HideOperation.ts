/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from '../previews';
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

  previewDag(dag: Dag): Dag {
    const hash = this.source.revset;
    const toHide = dag.descendants(hash);
    return dag.replaceWith(toHide, (h, c) => {
      const previewType = h === hash ? CommitPreview.HIDDEN_ROOT : CommitPreview.HIDDEN_DESCENDANT;
      return c && {...c, previewType};
    });
  }

  optimisticDag(dag: Dag): Dag {
    const hash = this.source.revset;
    const toHide = dag.descendants(hash);
    const toCleanup = dag.parents(hash);
    // If the head is being hidden, we need to move the head to the parent.
    const newHead = [];
    if (toHide.toHashes().some(h => dag.get(h)?.isHead == true)) {
      const parent = dag.get(hash)?.parents?.at(0);
      if (parent && dag.has(parent)) {
        newHead.push(parent);
      }
    }
    return dag
      .remove(toHide)
      .replaceWith(newHead, (_h, c) => {
        return c && {...c, isHead: true, previewType: CommitPreview.GOTO_DESTINATION};
      })
      .cleanup(toCleanup);
  }
}
