/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from '../previews';
import type {ExactRevset, SucceedableRevset} from '../types';

import {latestSuccessor} from '../SuccessionTracker';
import {CommitPreview} from '../previews';
import {Operation} from './Operation';

export class GotoOperation extends Operation {
  constructor(private destination: SucceedableRevset | ExactRevset) {
    super('GotoOperation');
  }

  static opName = 'Goto';

  getArgs() {
    const args = ['goto', '--rev', this.destination];
    return args;
  }

  optimisticDag(dag: Dag): Dag {
    const headCommitHash = dag.resolve('.')?.hash;
    if (headCommitHash == null) {
      return dag;
    }
    const dest = dag.resolve(latestSuccessor(dag, this.destination));
    const src = dag.get(headCommitHash);
    if (dest == null || src == null || dest.hash === src.hash) {
      return dag;
    }
    return dag.replaceWith([src.hash, dest.hash], (h, c) => {
      const isDest = h === dest.hash;
      const previewType = isDest
        ? CommitPreview.GOTO_DESTINATION
        : CommitPreview.GOTO_PREVIOUS_LOCATION;
      return c && {...c, isHead: isDest, previewType};
    });
  }
}
