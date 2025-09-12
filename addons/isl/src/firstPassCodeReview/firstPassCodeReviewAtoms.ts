/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {localStorageBackedAtomFamily} from '../jotaiUtils';
import type {Hash} from '../types';
import type {CodeReviewIssue} from './types';

export const firstPassCommentData = localStorageBackedAtomFamily<Hash, CodeReviewIssue[]>(
  'isl.first-pass-comments:',
  () => [],
);
