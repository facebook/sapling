/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  DiffId,
  DiffSummary,
  Disposable,
  Result,
  OperationCommandProgressReporter,
} from 'isl/src/types';

type DiffSummaries = Map<DiffId, DiffSummary>;
/**
 * API to fetch data from Remote Code Review system, like GitHub and Phabricator.
 */
export interface CodeReviewProvider {
  triggerDiffSummariesFetch(diffs: Array<DiffId>): unknown;

  onChangeDiffSummaries(callback: (result: Result<DiffSummaries>) => unknown): Disposable;

  /** Run a command not handled within sapling, such as a separate submit handler */
  runExternalCommand?(
    cwd: string,
    args: Array<string>,
    onProgress: OperationCommandProgressReporter,
  ): Promise<void>;

  dispose: () => void;
}
