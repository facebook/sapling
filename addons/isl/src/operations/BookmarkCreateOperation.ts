/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from '../previews';
import type {ExactRevset, OptimisticRevset, SucceedableRevset} from '../types';

import {Operation} from './Operation';

export class BookmarkCreateOperation extends Operation {
  /**
   * @param bookmark local bookmark name to create. Should NOT be a remote bookmark or stable location.
   */
  constructor(
    private revset: SucceedableRevset | ExactRevset | OptimisticRevset,
    private bookmark: string,
  ) {
    super('BookmarkCreateOperation');
  }

  static opName = 'BookmarkCreate';

  getArgs() {
    return ['bookmark', this.bookmark, '--rev', this.revset];
  }

  optimisticDag(dag: Dag): Dag {
    const commit = dag.resolve(this.bookmark);
    if (commit) {
      return dag.replaceWith(commit.hash, (_h, c) =>
        c?.merge({
          bookmarks: [...commit.bookmarks, this.bookmark],
        }),
      );
    }
    return dag;
  }
}
