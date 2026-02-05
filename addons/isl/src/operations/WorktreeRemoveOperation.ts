/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';

/**
 * Operation to remove a git worktree using `sl wt rm`.
 */
export class WorktreeRemoveOperation extends Operation {
  static opName = 'WorktreeRemove';

  constructor(
    private path: string,
    private force: boolean = false,
  ) {
    super('WorktreeRemoveOperation');
  }

  getPath(): string {
    return this.path;
  }

  getArgs() {
    const args = ['wt', 'rm'];
    if (this.force) {
      args.push('--force');
    }
    args.push(this.path);
    return args;
  }

  getDescriptionForDisplay() {
    const name = this.path.split(/[/\\]/).pop() ?? this.path;
    return {
      description: `Remove worktree "${name}"`,
    };
  }
}
