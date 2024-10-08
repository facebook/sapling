/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Result} from '../../types';
import type {Comparison} from 'shared/Comparison';

type ContextId = {path: string; comparison: Comparison};

/**
 * Context used to render SplitDiffView
 */
export type Context = {
  id: ContextId;
  copy?: (s: string) => void;
  openFile?: () => unknown;
  openFileToLine?: (line: OneIndexedLineNumber) => unknown;
  collapsed: boolean;
  setCollapsed: (collapsed: boolean) => void;
  fetchAdditionalLines?(
    id: ContextId,
    start: OneIndexedLineNumber,
    numLines: number,
  ): Promise<Result<Array<string>>>;
  /**
   * Whether to render as a side-by-side diff view, or a unified view where deleted and added lines are interleaved.
   * TODO: make this controllable / configurable / responsive based on screen width
   */
  display: 'split' | 'unified';
};

export type OneIndexedLineNumber = Exclude<number, 0>;
