/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from '../previews';

import {Operation} from './Operation';

export class BookmarkDeleteOperation extends Operation {
  /**
   * @param bookmark local bookmark name to delete. Should NOT be a remote bookmark or stable location.
   */
  constructor(private bookmark: string) {
    super('BookmarkDeleteOperation');
  }

  static opName = 'BookmarkDelete';

  getArgs() {
    return ['bookmark', '--delete', this.bookmark];
  }

  optimisticDag(dag: Dag): Dag {
    const commit = dag.resolve(this.bookmark);
    if (commit) {
      return dag.replaceWith(commit.hash, (_h, c) =>
        c?.merge({
          bookmarks: commit.bookmarks.filter(b => b !== this.bookmark),
        }),
      );
    }
    return dag;
  }
}
