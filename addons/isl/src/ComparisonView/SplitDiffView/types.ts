/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RecoilValueReadOnly} from 'recoil';
import type {Comparison} from 'shared/Comparison';

export type LineRangeParams<Id> = {
  // 1-based line number.
  start: number;
  numLines: number;
  id: Id;
};

/**
 * Context used to render SplitDiffView
 */
export type Context = {
  /**
   * Arbitrary identifying information for a given SplitDiffView, usually
   * information like a hash or revset + path.
   */
  id: {path: string; comparison: Comparison};
  atoms: {
    lineRange: (
      params: LineRangeParams<{path: string; comparison: Comparison}>,
    ) => RecoilValueReadOnly<Array<string>>;
  };
  translate?: (s: string) => string;
  copy?: (s: string) => void;
  openFile?: () => unknown;
  openFileToLine?: (line: OneIndexedLineNumber) => unknown;
  collapsed: boolean;
  setCollapsed: (collapsed: boolean) => void;
};

export type OneIndexedLineNumber = Exclude<number, 0>;
