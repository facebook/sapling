/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffSide} from './generated/graphql';

import {atom} from 'jotai';

export type ReviewCommentRange = {
  anchorLine: number;
  startLine: number;
  endLine: number;
  path: string;
  side: DiffSide;
};

/**
 * The contiguous code range selected for a new inline review comment.
 */
export const reviewCommentRangeAtom = atom<ReviewCommentRange | null>(null);
