/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {OperationDescription} from './Operation';

import {Operation} from './Operation';

export class NopOperation extends Operation {
  static opName = 'Nop';

  constructor(private durationSeconds = 2) {
    super('NopOperation');
  }

  getDescriptionForDisplay(): OperationDescription | undefined {
    return {
      description: `sleep ${this.durationSeconds}`,
    };
  }

  getArgs() {
    return [
      'debugprogress',
      '3',
      '--with-output',
      '1',
      '--sleep',
      String(Math.floor((this.durationSeconds * 1000) / 3)),
    ];
  }
}
