/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';

export class CommitCloudChangeWorkspaceOperation extends Operation {
  static opName = 'CommitCloudChangeWorkspace';

  constructor(private workspaceName: string) {
    super('CommitCloudChangeWorkspaceOperation');
  }

  getArgs() {
    return ['cloud', 'switch', '-w', this.workspaceName];
  }
}
