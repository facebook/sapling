/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MergeConflicts} from '../types';

import {Operation} from './Operation';

export class AbortMergeOperation extends Operation {
  constructor(private conflicts: MergeConflicts) {
    super();
  }

  static opName = 'Abort';

  // `sl abort` isn't a real command like `sl continue` is.
  // however, the merge conflict data we've fetched includes the command to abort
  getArgs() {
    if (this.conflicts.toAbort == null) {
      // if conflicts are still loading we don't know the right command...
      // just try `rebase --abort`...
      return ['rebase', '--abort'];
    }
    return this.conflicts.toAbort.split(' ');
  }
}
