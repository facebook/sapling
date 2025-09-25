/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/* Format is subject to change */
export type CodeReviewResult = {
  reviews: Array<{
    reviewerName: string;
    codeIssues: Array<CodeReviewIssue>;
  }>;
};

export type CodeReviewIssue = {
  issueID: string;
  filepath: string;
  description: string;
  startLine: number;
  endLine: number;
  severity: 'high' | 'medium' | 'low';
};

export type CodeReviewProgressStatus = 'running' | 'success' | 'error';
