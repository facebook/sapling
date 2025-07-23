/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommandArg, RepoRelativePath} from '../types';

import {Operation} from './Operation';

export class ResolveInExternalMergeToolOperation extends Operation {
  constructor(
    private tool: string,
    private filePath?: RepoRelativePath,
  ) {
    super('ResolveInExternalMergeToolOperation');
  }

  static opName = 'ResolveInExternalMergeToolOperation';

  getArgs() {
    const args: Array<CommandArg> = [
      'resolve',
      '--tool',
      this.tool,
      // skip merge drivers, since we're just looking to resolve in the UI.
      '--skip',
    ];

    if (this.filePath) {
      args.push({
        type: 'repo-relative-file' as const,
        path: this.filePath,
      });
    } else {
      args.push('--all');
    }
    return args;
  }
}
