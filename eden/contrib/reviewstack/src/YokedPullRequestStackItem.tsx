/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StackPullRequestFragment} from './generated/graphql';

import BulletItems from './BulletItems';
import YokedCommentCount from './YokedCommentCount';
import YokedPullRequestStateLabel from './YokedPullRequestStateLabel';
import useNavigateToPullRequest from './useNavigateToPullRequest';
import {formatISODateShort} from './utils';
import {ActionList, Box, Text} from '@primer/react';
import {
  CommentIcon,
  CommentDiscussionIcon,
  GitPullRequestIcon,
  CalendarIcon,
} from '@primer/octicons-react';
import React from 'react';
import cn from 'classnames';

function pad(num: number | string, size: number): string {
  num = num.toString();
  while (num.length < size) num = '0' + num;
  return num;
}

type Props = {
  isSelected: boolean;
  stackIndex: number;
} & StackPullRequestFragment;

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PullRequestStackItem({
  isSelected,
  stackIndex,
  comments,
  number,
  reviewDecision,
  state,
  title,
  updatedAt,
}: Props): React.ReactElement {
  const navigateToPullRequest = useNavigateToPullRequest();

  return (
    <div className={cn('stack-item', {active: isSelected})}>
      <div className="stack-item-order">{pad(stackIndex + 1, 2)}</div>
      <div className="stack-item-info">
        <div className="stack-item-title">
          <span className="stack-item-name">{title}</span>
          <span className="stack-item-id">{`#${number}`}</span>
        </div>
        <div className="stack-item-description">
          <YokedPullRequestStateLabel
            plaintext={true}
            reviewDecision={reviewDecision ?? null}
            state={state}
          />
          <span>{'\u30FB'}</span>
          <CalendarIcon size={12} />
          <span>{formatISODateShort(updatedAt, false)}</span>
          <span>{'\u30FB'}</span>
          <CommentIcon size={12} />
          {/* TODO(dk): comments.totalCount is broken for some reason (even in original Sapling app) */}
          <span>{comments.totalCount}</span>
        </div>
      </div>
      <button className="stack-item-handle" onClick={() => navigateToPullRequest(number)} />
    </div>
  );

  // return (
  //   <Box fontSize={0}>
  //     <Box overflow="hidden" sx={{textOverflow: 'ellipsis'}}>
  //       <Text fontWeight="bold" fontSize={1} whiteSpace="nowrap">
  //         {title}
  //       </Text>
  //     </Box>
  //     <PullRequestStateLabel
  //       reviewDecision={reviewDecision ?? null}
  //       state={state}
  //       variant="small"
  //     />
  //     <Text>#{number}</Text>
  //     {formatISODate(updatedAt, false)}
  //     <CommentCount count={comments.totalCount} />
  //   </Box>
  // );
});
