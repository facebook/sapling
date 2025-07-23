/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MergeConflicts} from '../types';

import {Operation} from './Operation';

export class AbortMergeOperation extends Operation {
  constructor(
    private conflicts: MergeConflicts,
    private isPartialAbort: boolean,
  ) {
    super('AbortMergeOperation');
  }

  static opName = 'Abort';

  // `sl abort` isn't a real command like `sl continue` is.
  // however, the merge conflict data we've fetched includes the command to abort
  getArgs() {
    if (this.isPartialAbort) {
      // only rebase supports partial aborts
      return ['rebase', '--quit'];
    }
    if (this.conflicts.toAbort == null) {
      // if conflicts are still loading we don't know the right command...
      // just try `rebase --abort`...
      return ['rebase', '--abort'];
    }
    return this.conflicts.toAbort.split(' ');
  }

  // It's tempting to add makeOptimisticMergeConflictsApplier to `abort`,
  // but hiding optimistic conflicts may reveal temporary uncommitted changes
  // we could use optimistic uncommitted changes to hide those as well,
  // but it gets complicated. More robust is to just show a spinner on the abort button instead.
  // Abort should be relatively quick.
  // TODO: if this is a slow point in workflows, we could make this experience smoother.
}
