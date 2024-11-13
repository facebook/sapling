/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommandArg, RepoRelativePath} from '../types';

import {Operation} from './Operation';

export class CommitBaseOperation extends Operation {
  constructor(public message: string, protected filesPathsToCommit?: Array<RepoRelativePath>) {
    super(filesPathsToCommit ? 'CommitFileSubsetOperation' : 'CommitOperation');
  }

  static opName = 'Commit';

  getArgs() {
    const args: Array<CommandArg> = ['commit', '--addremove', '--message', this.message];
    if (this.filesPathsToCommit) {
      args.push(
        ...this.filesPathsToCommit.map(file =>
          // tag file arguments specialy so the remote repo can convert them to the proper cwd-relative format.
          ({
            type: 'repo-relative-file' as const,
            path: file,
          }),
        ),
      );
    }
    return args;
  }
}
