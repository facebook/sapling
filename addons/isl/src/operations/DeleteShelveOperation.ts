/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ShelvedChange} from '../types';

import {Operation} from './Operation';

export class DeleteShelveOperation extends Operation {
  constructor(private shelvedChange: ShelvedChange) {
    super('DeleteShelveOperation');
  }

  static opName = 'Unshelve';

  getArgs() {
    const args = ['shelve', '--delete', this.shelvedChange.name];
    return args;
  }
}
