/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PullRequestReviewDecision} from './generated/graphql';

import {pullRequestReviewDecisionLabel} from './utils';
import {Label} from '@primer/react';
import React from 'react';

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PullRequestReviewDecisionLabel({
  reviewDecision,
}: {
  reviewDecision?: PullRequestReviewDecision | null;
}): React.ReactElement | null {
  if (reviewDecision == null) {
    return null;
  }
  const {label, variant} = pullRequestReviewDecisionLabel(reviewDecision);

  return <Label variant={variant}>{label}</Label>;
});
