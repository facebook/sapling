/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Comparison} from 'shared/Comparison';

/**
 * Context used to render SplitDiffView
 */
export type Context = {
  id: {path: string; comparison: Comparison};
  copy?: (s: string) => void;
  openFile?: () => unknown;
  openFileToLine?: (line: OneIndexedLineNumber) => unknown;
  collapsed: boolean;
  setCollapsed: (collapsed: boolean) => void;
  supportsExpandingContext: boolean;
  /**
   * Whether to render as a side-by-side diff view, or a unified view where deleted and added lines are interleaved.
   * TODO: make this controllable / configurable / responsive based on screen width
   */
  display: 'split' | 'unified';
};

export type OneIndexedLineNumber = Exclude<number, 0>;
