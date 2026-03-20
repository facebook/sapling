/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from '../previews';
import type {ExactRevset, OptimisticRevset, SucceedableRevset} from '../types';

import {CommitPreview} from '../previews';
import {Operation} from './Operation';

type RevsetSource = SucceedableRevset | ExactRevset | OptimisticRevset;

export class HideOperation extends Operation {
  private sources: Array<RevsetSource>;

  constructor(source: RevsetSource | Array<RevsetSource>) {
    super('HideOperation');
    this.sources = Array.isArray(source) ? source : [source];
  }

  static opName = 'Hide';

  getArgs() {
    return ['hide', ...this.sources.map(source => ['--rev', source] as const).flat()];
  }

  private hashes(): string[] {
    return this.sources.map(source =>
      source.type === 'optimistic-revset' ? source.fake : source.revset,
    );
  }

  previewDag(dag: Dag): Dag {
    const hashes = this.hashes();
    const hashSet = new Set(hashes);
    const toHide = dag.descendants(hashes);
    return dag.replaceWith(toHide, (h, c) => {
      const previewType = hashSet.has(h)
        ? CommitPreview.HIDDEN_ROOT
        : CommitPreview.HIDDEN_DESCENDANT;
      return c?.merge({previewType});
    });
  }

  optimisticDag(dag: Dag): Dag {
    const hashes = this.hashes();
    const toHide = dag.descendants(hashes);
    const toCleanup = dag.parents(hashes);
    // If the head is being hidden, we need to move the head to the parent.
    const newHead: string[] = [];
    if (toHide.toHashes().some(h => dag.get(h)?.isDot == true)) {
      for (const hash of hashes) {
        const parent = dag.get(hash)?.parents?.at(0);
        if (parent && dag.has(parent) && !toHide.contains(parent)) {
          newHead.push(parent);
          break;
        }
      }
    }
    return dag
      .remove(toHide)
      .replaceWith(newHead, (_h, c) => {
        return c?.merge({isDot: true, previewType: CommitPreview.GOTO_DESTINATION});
      })
      .cleanup(toCleanup);
  }
}
