/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StackPullRequestFragment} from './generated/graphql';

import BulletItems from './BulletItems';
import CommentCount from './CommentCount';
import PullRequestStateLabel from './PullRequestStateLabel';
import useNavigateToPullRequest from './useNavigateToPullRequest';
import {formatISODate} from './utils';
import {ActionList, Box, Text} from '@primer/react';
import React from 'react';

type Props = {
  isSelected: boolean;
} & StackPullRequestFragment;

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PullRequestStackItem({
  isSelected,
  comments,
  number,
  reviewDecision,
  state,
  title,
  updatedAt,
}: Props): React.ReactElement {
  const navigateToPullRequest = useNavigateToPullRequest();

  return (
    <ActionList.Item
      onSelect={() => navigateToPullRequest(number)}
      selected={isSelected}
      sx={{display: 'flex', alignItems: 'center'}}>
      <Box fontSize={0}>
        <Box overflow="hidden" sx={{textOverflow: 'ellipsis'}}>
          <Text fontWeight="bold" fontSize={1} whiteSpace="nowrap">
            {title}
          </Text>
        </Box>
        <BulletItems>
          <PullRequestStateLabel
            reviewDecision={reviewDecision ?? null}
            state={state}
            variant="small"
          />
          <Text>#{number}</Text>
          {formatISODate(updatedAt, false)}
          <CommentCount count={comments.totalCount} />
        </BulletItems>
      </Box>
    </ActionList.Item>
  );
});
