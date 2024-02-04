/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';

export class CommitCloudCreateWorkspaceOperation extends Operation {
  static opName = 'CommitCloudCreateWorkspace';

  constructor(private workspaceName: string) {
    super('CommitCloudCreateWorkspaceOperation');
  }

  getArgs() {
    return ['cloud', 'switch', '--create', '-w', this.workspaceName];
  }
}
