/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {Comparison} from 'shared/Comparison';
import type {ThemeColor} from '../../theme';
import type {PullRequestReviewSide, Result} from '../../types';

type ContextId = {path: string; comparison: Comparison};

export type DiffViewMode = 'split' | 'unified';

export type DiffLineLocation = {
  path: string;
  /** End line for a range, or the only line for a single-line comment. */
  line: OneIndexedLineNumber;
  side: PullRequestReviewSide;
  /** First line for a multi-line comment. Omitted for a single-line comment. */
  startLine?: OneIndexedLineNumber;
  startSide?: PullRequestReviewSide;
};

/**
 * Context used to render SplitDiffView
 */
export type Context = {
  id: ContextId;
  copy?: (s: string) => void;
  openFile?: () => unknown;
  openFileToLine?: (line: OneIndexedLineNumber) => unknown;
  /** Start a new review comment on a line in the displayed patch. */
  onStartComment?: (location: DiffLineLocation) => unknown;
  /** Whether a line belongs to an existing review comment range. */
  isLineCommented?: (location: DiffLineLocation) => boolean;
  /** Render existing review threads or an active composer below a diff line. */
  renderLineAddon?: (location: DiffLineLocation) => ReactNode;
  collapsed: boolean;
  setCollapsed: (collapsed: boolean) => void;
  fetchAdditionalLines?(
    id: ContextId,
    start: OneIndexedLineNumber,
    numLines: number,
  ): Promise<Result<Array<string>>>;
  displayLineNumbers?: boolean;
  /** A React hook that gives a string value used as an effect dependency. If this value changes, the comparison will be considered invalidated and must be refreshed.
   * This is a hook so it can trigger rerenders. */
  useComparisonInvalidationKeyHook?: () => string;
  /** A React hook that returns the current theme color. This is a hook so it can trigger rerenders, but can use atom values. */
  useThemeHook: () => ThemeColor;
  /** Translation function for the current language. */
  t?: (s: string) => string;
  /**
   * Whether to render as a side-by-side diff view, or a unified view where deleted and added lines are interleaved.
   * TODO: make this controllable / configurable / responsive based on screen width
   */
  display: DiffViewMode;
};

export type OneIndexedLineNumber = Exclude<number, 0>;
