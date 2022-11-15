/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';

export class SetConfigOperation extends Operation {
  constructor(
    private scope: 'user' | 'local' | 'global',
    private configName: string,
    private value: string,
  ) {
    super();
  }

  static opName = 'Add';

  getArgs() {
    return ['config', `--${this.scope}`, this.configName, this.value];
  }
}
