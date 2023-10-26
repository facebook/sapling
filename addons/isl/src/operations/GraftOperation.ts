/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';

import {t} from '../i18n';
import {Operation} from './Operation';

/** Graft (copy) a commit onto the current commit. Like Rebasing, without affecting the original commit.
 * Useful for public commits. Note: Does not use latest successor by default, rather the exact revset. */
export class GraftOperation extends Operation {
  constructor(private source: Hash) {
    super('GraftOperation');
  }

  static opName = 'Graft';

  getArgs() {
    return ['graft', this.source];
  }

  getInitialInlineProgress(): Array<[string, string]> {
    // TODO: successions
    return [[this.source, t('grafting...')]];
  }

  // TODO: Optimistic State
}
