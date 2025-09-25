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
import type {CodeReviewIssue} from './types';

export const firstPassCommentData = atom<CodeReviewIssue[]>([]);

registerDisposable(
  firstPassCommentData,
  platform.aiCodeReview?.onDidChangeAIReviewComments(comments => {
    writeAtom(firstPassCommentData, comments);
  }) ?? {dispose: () => {}},
  import.meta.hot,
);
