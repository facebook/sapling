/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';
import {CommandRunner} from '../types';

/**
 * Operation to close a PR via `gh pr close`.
 * Used to close PRs below a merged PR in a stack (their changes are already in main).
 */
export class ClosePROperation extends Operation {
  static opName = 'ClosePR';

  constructor(
    private prNumber: number,
    private comment?: string,
  ) {
    super('RunOperation');
  }

  public runner = CommandRunner.CodeReviewProvider;

  getArgs(): string[] {
    const args = ['pr', 'close', String(this.prNumber)];

    if (this.comment) {
      args.push('--comment', this.comment);
    }

    return args;
  }

  getDescriptionForDisplay() {
    return {
      description: `Closing PR #${this.prNumber}`,
      tooltip: `gh pr close ${this.prNumber}${this.comment ? ` --comment "${this.comment}"` : ''}`,
    };
  }
}
