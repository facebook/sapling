/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';

export class CreateEmptyInitialCommitOperation extends Operation {
  static opName = 'CreateEmptyInitialCommit';

  constructor() {
    super('CreateEmptyInitialCommit');
  }

  getArgs() {
    return ['commit', '--config', 'ui.allowemptycommit=true', '--message', 'Initial Commit'];
  }
}
