/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {PullRequestReviewDecision, PullRequestState} from './generated/graphql';

type Status = 'pullClosed' | 'pullMerged' | 'pullOpened';

export default function pullRequestStatusAndLabel(
  state: PullRequestState,
  reviewDecision: PullRequestReviewDecision | null | undefined,
  isDraft: boolean,
): {
  status: Status;
  label: string;
  color?: string;
} {
  switch (state) {
    case PullRequestState.Closed:
      return {status: 'pullClosed', label: 'Closed'};
    case PullRequestState.Merged:
      return {status: 'pullMerged', label: 'Merged'};
    case PullRequestState.Open: {
      const status = 'pullOpened';
      if (isDraft) {
        return {status, label: 'Draft Review', color: 'fg.muted'};
      }
      switch (reviewDecision) {
        case PullRequestReviewDecision.Approved:
          return {status, label: 'Approved', color: 'success.fg'};
        case PullRequestReviewDecision.ChangesRequested:
          return {status, label: 'Changes Req.', color: 'danger.fg'};
        case PullRequestReviewDecision.ReviewRequired:
        case null:
        case undefined:
          return {status, label: 'Review Required', color: 'attention.fg'};
      }
    }
  }
}
