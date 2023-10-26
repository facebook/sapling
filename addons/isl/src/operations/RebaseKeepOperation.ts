/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Revset} from '../types';

import {SucceedableRevset} from '../types';
import {Operation} from './Operation';

/** Like rebase, but leave the source in place, and don't rebase children.
 * Behaves more like "Graft" than rebase, but without going to the result. Useful for copying public commits.
 * Note: does not use the latest successor by default, rather the exact source revset. */
export class RebaseKeepOperation extends Operation {
  constructor(protected source: Revset, protected destination: Revset) {
    super('RebaseKeepOperation');
  }

  static opName = 'Rebase (keep)';

  getArgs() {
    return [
      'rebase',
      '--keep',
      '--rev',
      this.source,
      '--dest',
      SucceedableRevset(this.destination),
    ];
  }

  // TODO: Support optimistic state. Presently not an issue because its use case in "Download Commits"
  // doesn't support optimistic state anyway.
}
