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
    private useWorktree: boolean = false,
    private worktreeName?: string,
  ) {
    super('PullStackOperation');
  }

  getArgs() {
    const args = ['pr', 'get', String(this.prNumber)];
    if (this.useWorktree) {
      args.push('--wt');
      if (this.worktreeName) {
        args.push('--wt-name', this.worktreeName);
      }
    } else if (this.goto) {
      args.push('--goto');
    }
    return args;
  }

  getDescriptionForDisplay() {
    if (this.useWorktree) {
      const wtName = this.worktreeName ?? `pr-${this.prNumber}`;
      return {
        description: `Pull stack for PR #${this.prNumber} into worktree "${wtName}"`,
      };
    }
    return {
      description: `Pull stack for PR #${this.prNumber}`,
    };
  }
}
