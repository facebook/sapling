/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RecoilValueReadOnly} from 'recoil';

export type LineRangeParams<Id> = {
  // 1-based line number.
  start: number;
  numLines: number;
  id: Id;
};

/**
 * Context used to render SplitDiffView
 */
export type Context<T> = {
  /**
   * Arbitrary identifying information for a given SplitDiffView, usually
   * information like a hash or revset + path.
   */
  id: T;
  atoms: {
    lineRange: (params: LineRangeParams<T>) => RecoilValueReadOnly<Array<string>>;
  };
  translate?: (s: string) => string;
};
