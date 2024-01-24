/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from '../previews';
import type {ExactRevset, SucceedableRevset} from '../types';

import {latestSuccessor} from '../SuccessionTracker';
import {Operation} from './Operation';

export class BulkRebaseOperation extends Operation {
  constructor(
    private sources: Array<SucceedableRevset>,
    private destination: ExactRevset | SucceedableRevset,
  ) {
    super('BulkRebaseOperation');
  }

  static opName = 'Bulk rebase commits';

  getArgs() {
    return [
      'rebase',
      ...this.sources.map(source => ['--rev', source]).flat(),
      '-d',
      this.destination,
    ];
  }

  optimisticDag(dag: Dag): Dag {
    const dest = dag.resolve(latestSuccessor(dag, this.destination))?.hash;
    const source = this.sources.map(s => latestSuccessor(dag, s));
    return dag.rebase(source, dest);
  }
}
