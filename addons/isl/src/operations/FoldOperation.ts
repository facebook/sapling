/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag, WithPreviewType} from '../previews';
import type {CommitInfo} from '../types';

import {CommitPreview} from '../previews';
import {exactRevset} from '../types';
import {firstLine} from '../utils';
import {Operation} from './Operation';

/**
 * Returns [bottom, top] of an array.
 */
function ends<T>(range: Array<T>): [T, T] {
  return [range[0], range[range.length - 1]];
}

export class FoldOperation extends Operation {
  constructor(private foldRange: Array<CommitInfo>, newMessage: string) {
    super('FoldOperation');
    this.newTitle = firstLine(newMessage);
    this.newDescription = newMessage.substring(firstLine(newMessage).length + 1);
  }
  private newTitle: string;
  private newDescription: string;

  static opName = 'Fold';

  getArgs() {
    const [bottom, top] = ends(this.foldRange);
    return [
      'fold',
      '--exact',
      exactRevset(`${bottom.hash}::${top.hash}`),
      '--message',
      `${this.newTitle}\n${this.newDescription}`,
    ];
  }

  public getFoldRange(): Array<CommitInfo> {
    return this.foldRange;
  }
  public getFoldedMessage(): [string, string] {
    return [this.newTitle, this.newDescription];
  }

  previewDag(dag: Dag): Dag {
    return this.calculateDagPreview(dag, true);
  }

  optimisticDag(dag: Dag): Dag {
    return this.calculateDagPreview(dag, false);
  }

  private calculateDagPreview(dag: Dag, isPreview: boolean): Dag {
    const hashes = this.foldRange.map(info => info.hash);
    const top = hashes.at(-1);
    const parents = dag.get(hashes.at(0))?.parents;
    if (top == null || parents == null) {
      return dag;
    }
    const hash = getFoldRangeCommitHash(this.foldRange, isPreview);
    const bookmarks = hashes.flatMap(h => dag.get(h)?.bookmarks ?? []).sort();
    return dag
      .replaceWith(hashes, (h, c) => {
        if (h !== top && c == null) {
          return undefined;
        }
        return {
          ...c,
          date: new Date(),
          hash,
          bookmarks,
          title: this.newTitle,
          description: this.newDescription,
          previewType: isPreview ? CommitPreview.FOLD_PREVIEW : CommitPreview.FOLD,
          parents,
        } as CommitInfo & WithPreviewType;
      })
      .replaceWith(dag.children(top), (_h, c) => {
        return c && {...c, parents: c.parents.map(p => (p === top ? hash : p))};
      });
  }
}

export const FOLD_COMMIT_PREVIEW_HASH_PREFIX = 'OPTIMISTIC_FOLDED_PREVIEW_';
export const FOLD_COMMIT_OPTIMISTIC_HASH_PREFIX = 'OPTIMISTIC_FOLDED_';
export function getFoldRangeCommitHash(range: Array<CommitInfo>, isPreview: boolean): string {
  const [bottom, top] = ends(range);
  return (
    (isPreview ? FOLD_COMMIT_PREVIEW_HASH_PREFIX : FOLD_COMMIT_OPTIMISTIC_HASH_PREFIX) +
    `${bottom.hash}:${top.hash}`
  );
}
