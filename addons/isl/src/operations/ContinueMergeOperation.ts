/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';

export class ContinueOperation extends Operation {
  static opName = 'Continue';

  constructor() {
    super('ContinueMergeOperation');
  }

  getArgs() {
    return ['continue'];
  }

  // It's tempting to add makeOptimisticMergeConflictsApplier to `continue`,
  // but we don't know if we'll hit additional merge conflicts,
  // and we don't want our detection of conflicts to not work.
  // Instead of optimistically hiding the conflicts, we show a spinner on the continue button.
}
