/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from '../previews';
import type {ExactRevset, SucceedableRevset} from '../types';

import {latestSuccessor} from '../SuccessionTracker';
import {exactRevset} from '../types';
import {Operation} from './Operation';

export class RebaseAllDraftCommitsOperation extends Operation {
  constructor(
    private timeRangeDays: number | undefined,
    private destination: ExactRevset | SucceedableRevset,
  ) {
    super('RebaseAllDraftCommitsOperation');
  }

  static opName = 'Rebase all draft commits';

  getArgs() {
    return [
      'rebase',
      '-s',
      exactRevset(
        this.timeRangeDays == null ? 'draft()' : `draft() & date(-${this.timeRangeDays})`,
      ),
      '-d',
      this.destination,
    ];
  }

  optimisticDag(dag: Dag): Dag {
    const dest = dag.resolve(latestSuccessor(dag, this.destination))?.hash;
    const draft = dag.draft();
    return dag.rebase(draft, dest);
  }
}
