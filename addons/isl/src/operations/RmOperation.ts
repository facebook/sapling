/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommandArg, RepoRelativePath} from '../types';

import {Operation} from './Operation';

export class RmOperation extends Operation {
  constructor(
    private filePath: RepoRelativePath,
    private force: boolean,
  ) {
    super('RmOperation');
  }

  static opName = 'Rm';

  getArgs() {
    const args: Array<CommandArg> = ['rm'];
    if (this.force) {
      args.push('-f');
    }
    args.push({
      type: 'repo-relative-file' as const,
      path: this.filePath,
    });
    return args;
  }
}
