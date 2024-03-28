/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PullRequestReviewDecision} from './generated/graphql';

import {PullRequestState} from './generated/graphql';
import {pullRequestReviewDecisionLabel} from './utils';
import {StateLabel, Token, Link, StyledOcticon, CircleOcticon} from '@primer/react';
import {GitPullRequestIcon, GitMergeIcon, GitPullRequestClosedIcon} from '@primer/octicons-react';

type Status = 'pullClosed' | 'pullMerged' | 'pullOpened';

export default function PullRequestStateLabel({
  reviewDecision,
  state,
  variant = 'small',
  plaintext = false,
}: {
  reviewDecision: PullRequestReviewDecision | null;
  state: PullRequestState;
  variant?: 'small' | 'normal';
  plaintext?: boolean | undefined;
}) {
  const {status, label, color} = statusAndLabel(state, reviewDecision);
  const tagIcon = {
    [PullRequestState.Closed]: GitPullRequestClosedIcon,
    [PullRequestState.Merged]: GitMergeIcon,
    [PullRequestState.Open]: GitPullRequestIcon,
  }[state];
  if (plaintext) {
    // return <>{label}</>;
    return (
      <>
        <StyledOcticon icon={tagIcon} size={12} /> {label}
      </>
    );
  }
  return (
    <Token
      size="large"
      text={label}
      title={`Pull request is ${label}`}
      leadingVisual={() => <StyledOcticon icon={tagIcon} size={16} sx={{marginLeft: '0'}} />}
      sx={{
        color: '#fff',
        backgroundColor: color,
        borderColor: color,
        cursor: 'pointer',
        paddingLeft: '8px',
        paddingRight: '8px',
      }}
    />
  );
  return (
    <StateLabel
      status={status}
      variant={variant}
      sx={{backgroundColor: color, paddingLeft: '8px', paddingRight: '10px'}}>
      {label}
    </StateLabel>
  );
}

function statusAndLabel(
  state: PullRequestState,
  reviewDecision: PullRequestReviewDecision | null,
): {
  status: Status;
  label: string;
  color?: string;
} {
  switch (state) {
    case PullRequestState.Closed: {
      const status = 'pullClosed';
      if (reviewDecision === null) {
        return {status, label: 'Closed', color: 'danger.fg'};
      }
      const {label, variant} = pullRequestReviewDecisionLabel(reviewDecision);
      return {status, label, color: `${variant}.fg`};
    }

    case PullRequestState.Merged: {
      const status = 'pullMerged';
      if (reviewDecision === null) {
        return {status, label: 'Merged', color: 'done.fg'};
      }
      const {label, variant} = pullRequestReviewDecisionLabel(reviewDecision);
      return {status, label, color: `${variant}.fg`};
    }

    case PullRequestState.Open: {
      const status = 'pullOpened';
      if (reviewDecision === null) {
        return {status, label: 'Open', color: 'success.fg'};
      }
      const {label, variant} = pullRequestReviewDecisionLabel(reviewDecision);
      return {status, label, color: `${variant}.fg`};
    }
  }
}
