/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ImportStack} from 'shared/types/stack';

import {Operation} from './Operation';

export class ImportStackOperation extends Operation {
  static opName = 'StackEdit';

  constructor(private importStack: Readonly<ImportStack>) {
    super('ImportStackOperation');
  }

  getArgs() {
    return ['debugimportstack'];
  }

  getStdin() {
    return JSON.stringify(this.importStack);
  }
}
