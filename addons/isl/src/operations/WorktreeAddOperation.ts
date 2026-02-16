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

  /** Get the commit hash this worktree was created for */
  getCommit(): string {
    return this.commit;
  }

  /** Get the expected worktree name (short hash if not specified) */
  getWorktreeName(): string {
    return this.name ?? this.commit.slice(0, 8);
  }

  getArgs() {
    const args = ['wt', 'add'];
    // sl wt add [NAME] [COMMIT] — both positional args required.
    // Without NAME, the commit hash is misinterpreted as the name
    // and the worktree checks out the current commit instead.
    args.push(this.name ?? this.commit.slice(0, 8));
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
