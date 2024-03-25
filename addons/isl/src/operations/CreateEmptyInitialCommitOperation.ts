/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommandArg} from '../types';

import {Operation} from './Operation';

export class CreateEmptyInitialCommitOperation extends Operation {
  static opName = 'CreateEmptyInitialCommit';

  constructor() {
    super('CreateEmptyInitialCommit');
  }

  getArgs(): Array<CommandArg> {
    return [
      'commit',
      {type: 'config', key: 'ui.allowemptycommit', value: 'true'},
      '--message',
      'Initial Commit',
    ];
  }
}
