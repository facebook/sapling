/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';

/**
 * Operation to create a new git worktree for a commit using `sl wt add`.
 *
 * Worktrees allow checking out multiple commits simultaneously in separate
 * directories, enabling parallel development workflows.
 */
export class WorktreeAddOperation extends Operation {
  static opName = 'WorktreeAdd';

  constructor(
    private commit: string,
    private name?: string,
  ) {
    super('WorktreeAddOperation');
  }

  getArgs() {
    const args = ['wt', 'add'];
    if (this.name) {
      args.push(this.name);
    }
    args.push(this.commit);
    return args;
  }

  getDescriptionForDisplay() {
    const shortHash = this.commit.slice(0, 8);
    const displayName = this.name ?? shortHash;
    return {
      description: `Create worktree "${displayName}" for ${shortHash}`,
    };
  }
}
