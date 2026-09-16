/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PullRequestReviewDecision} from './generated/graphql';

import {PullRequestState} from './generated/graphql';
import {pullRequestReviewDecisionLabel} from './utils';
import {StateLabel} from '@primer/react';

type Status = 'pullClosed' | 'pullMerged' | 'pullOpened';

export default function PullRequestStateLabel({
  isDraft = false,
  reviewDecision,
  state,
  variant = 'normal',
}: {
  isDraft?: boolean;
  reviewDecision: PullRequestReviewDecision | null;
  state: PullRequestState;
  variant?: 'small' | 'normal';
}) {
  const {status, label, color} = statusAndLabel(state, reviewDecision, isDraft);
  return (
    <StateLabel status={status} variant={variant} sx={{backgroundColor: color}}>
      {label}
    </StateLabel>
  );
}

function statusAndLabel(
  state: PullRequestState,
  reviewDecision: PullRequestReviewDecision | null,
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
      if (reviewDecision === null) {
        return {status, label: 'Open', color: 'success.fg'};
      }
      const {label, variant} = pullRequestReviewDecisionLabel(reviewDecision);
      return {status, label, color: `${variant}.fg`};
    }
  }
}
