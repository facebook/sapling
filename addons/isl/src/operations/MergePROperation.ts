/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';
import {CommandRunner} from '../types';

export type MergeStrategy = 'merge' | 'squash' | 'rebase';

/**
 * Operation to merge a PR via `gh pr merge`.
 * Supports strategy selection (merge, squash, rebase) per MRG-02.
 */
export class MergePROperation extends Operation {
  static opName = 'MergePR';

  constructor(
    private prNumber: number,
    private strategy: MergeStrategy,
    private deleteBranch: boolean = false,
  ) {
    super('RunOperation');
  }

  // Use gh CLI for merge (not sl)
  // CommandRunner.CodeReviewProvider is defined in types.ts at line 509:
  // CodeReviewProvider = 'codeReviewProvider'
  public runner = CommandRunner.CodeReviewProvider;

  getArgs(): string[] {
    const args = ['pr', 'merge', String(this.prNumber)];

    // Add strategy flag
    args.push(`--${this.strategy}`);

    // Optionally delete branch after merge
    if (this.deleteBranch) {
      args.push('--delete-branch');
    }

    // Non-interactive mode
    args.push('--yes');

    return args;
  }

  getDescriptionForDisplay() {
    const strategyLabel = {
      merge: 'merge commit',
      squash: 'squash and merge',
      rebase: 'rebase and merge',
    }[this.strategy];

    return {
      description: `Merging PR #${this.prNumber} (${strategyLabel})`,
      tooltip: `gh pr merge ${this.prNumber} --${this.strategy}${this.deleteBranch ? ' --delete-branch' : ''} --yes`,
    };
  }
}
