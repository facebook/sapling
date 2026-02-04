/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';
import {CommandRunner} from '../types';

/**
 * Sync a PR branch with the latest base branch (main) using GitHub CLI.
 * Uses `gh pr update-branch --rebase` which:
 * - Fetches latest from remote
 * - Rebases PR commits onto updated base
 * - Force-pushes to PR branch
 *
 * May trigger merge conflicts that need manual resolution.
 */
export class SyncPROperation extends Operation {
  static opName = 'Sync PR';

  // Public so SyncProgress can access it to match operations to PRs
  constructor(public prNumber: string) {
    super('RunOperation');
  }

  // Use gh CLI for sync (not sl)
  public runner = CommandRunner.CodeReviewProvider;

  getArgs(): string[] {
    // Use gh CLI to update PR branch
    // --rebase flag uses rebase instead of merge commit
    return ['pr', 'update-branch', this.prNumber, '--rebase'];
  }

  getInitialInlineProgress(): Array<[string, string]> {
    return [[this.prNumber, 'syncing with main...']];
  }

  getDescriptionForDisplay() {
    return {
      description: `Syncing PR #${this.prNumber} with main`,
      tooltip: `gh pr update-branch ${this.prNumber} --rebase`,
    };
  }
}
