/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';

/**
 * Operation to pull an entire PR stack using `sl pr get`.
 *
 * This operation discovers and imports a full stack of PRs based on the
 * stack information in the PR body.
 */
export class PullStackOperation extends Operation {
  static opName = 'PullStack';

  constructor(
    private prNumber: number,
    private goto: boolean = true,
  ) {
    super('PullStackOperation');
  }

  getArgs() {
    const args = ['pr', 'get', String(this.prNumber)];
    if (this.goto) {
      args.push('--goto');
    }
    return args;
  }

  getDescriptionForDisplay() {
    return {
      description: `Pull stack for PR #${this.prNumber}`,
    };
  }
}
