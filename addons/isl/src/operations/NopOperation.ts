/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {OperationDescription} from './Operation';

import {Operation} from './Operation';

export class NopOperation extends Operation {
  static opName = 'Pull';

  constructor(private durationSeconds = 2) {
    super('NopOperation');
  }

  getDescriptionForDisplay(): OperationDescription | undefined {
    return {
      description: `sleep ${this.durationSeconds}`,
    };
  }

  getArgs() {
    return ['debugshell', '-c', `__import__('time').sleep(${this.durationSeconds})`];
  }
}
