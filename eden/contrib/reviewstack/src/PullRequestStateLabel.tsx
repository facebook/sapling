/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PullRequestReviewDecision, PullRequestState} from './generated/graphql';

import pullRequestStatusAndLabel from './pullRequestStatusAndLabel';
import {StateLabel} from '@primer/react';

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
  const {status, label, color} = pullRequestStatusAndLabel(state, reviewDecision, isDraft);
  return (
    <StateLabel status={status} variant={variant} sx={{backgroundColor: color}}>
      {label}
    </StateLabel>
  );
}
