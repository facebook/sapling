/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ExactRevset, OptimisticRevset, SucceedableRevset} from '../types';

import {Operation} from './Operation';

export class PushOperation extends Operation {
  static opName = 'Push';

  constructor(
    private topOfStackRev: SucceedableRevset | ExactRevset | OptimisticRevset,
    private toBranchName: string,
    private destination?: string,
  ) {
    super('PushOperation');
  }

  getArgs() {
    const args = ['push', '--rev', this.topOfStackRev, '--to', this.toBranchName];
    if (this.destination) {
      args.push(this.destination);
    }
    return args;
  }
}
