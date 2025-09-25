/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom} from 'jotai';
import {writeAtom} from '../jotaiUtils';
import platform from '../platform';
import {registerDisposable} from '../utils';
import type {CodeReviewIssue, CodeReviewProgressStatus} from './types';

/**
 * Atom that stores the current status of the AI code review.
 */
export const codeReviewStatusAtom = atom<CodeReviewProgressStatus | null>(null);

/**
 * Atom that stores comments for the current review.
 */
export const firstPassCommentData = atom<CodeReviewIssue[]>([]);

export const firstPassCommentDataCount = atom(get => get(firstPassCommentData).length);

export const firstPassCommentError = atom<Error | undefined>(undefined);

/**
 * Derived atom that maps comments by file path.
 * The resulting object has file paths as keys and arrays of CodeReviewIssue as values.
 */
export const commentsByFilePathAtom = atom(get => {
  const comments = get(firstPassCommentData);
  return comments.reduce<Record<string, CodeReviewIssue[]>>((acc, comment) => {
    if (!acc[comment.filepath]) {
      acc[comment.filepath] = [];
    }
    acc[comment.filepath].push(comment);
    return acc;
  }, {});
});

registerDisposable(
  firstPassCommentData,
  platform.aiCodeReview?.onDidChangeAIReviewComments(comments => {
    writeAtom(firstPassCommentData, comments);
  }) ?? {dispose: () => {}},
  import.meta.hot,
);
