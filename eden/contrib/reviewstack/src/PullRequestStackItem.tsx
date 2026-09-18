/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StackPullRequestFragment} from './generated/graphql';
import type {StackGraphPosition} from './pullRequestStackGraph';

import BulletItems from './BulletItems';
import CommentCount from './CommentCount';
import PullRequestStateLabel from './PullRequestStateLabel';
import useNavigateToPullRequest from './useNavigateToPullRequest';
import {formatISODate} from './utils';
import {ActionList, Box, Text} from '@primer/react';
import React from 'react';

type Props = {
  graphPosition?: StackGraphPosition;
  isSelected: boolean;
} & StackPullRequestFragment;

const GRAPH_COLUMN_WIDTH = 24;
const GRAPH_NODE_CENTER_X = 8;
const GRAPH_NODE_CENTER_Y = 21;
const GRAPH_NODE_RADIUS = 4;
const GRAPH_ROW_OVERLAP = 12;

function StackGraphPrefix({
  incomingLanes,
  nodeLane,
  outgoing,
  throughLanes,
}: StackGraphPosition): React.ReactElement {
  const nodeX = GRAPH_NODE_CENTER_X + nodeLane * GRAPH_COLUMN_WIDTH;
  const width = nodeX + GRAPH_NODE_RADIUS;

  return (
    <Box
      aria-hidden="true"
      data-incoming-lanes={incomingLanes.join(',')}
      data-node-lane={nodeLane}
      data-outgoing={outgoing ? 'true' : 'false'}
      data-testid="stack-graph-prefix"
      data-through-lanes={throughLanes.join(',')}
      minWidth={`${width}px`}
      position="relative"
      marginRight={2}
      sx={{alignSelf: 'stretch', color: 'accent.fg', overflow: 'visible'}}
      width={`${width}px`}>
      <svg
        height={`calc(100% + ${GRAPH_ROW_OVERLAP * 2}px)`}
        width={width}
        style={{
          left: 0,
          overflow: 'visible',
          pointerEvents: 'none',
          position: 'absolute',
          top: -GRAPH_ROW_OVERLAP,
        }}>
        <g fill="currentColor" stroke="currentColor" strokeLinecap="round" strokeWidth={2}>
          {throughLanes.map(lane => (
            <line
              key={lane}
              data-testid="stack-graph-through"
              x1={GRAPH_NODE_CENTER_X + lane * GRAPH_COLUMN_WIDTH}
              x2={GRAPH_NODE_CENTER_X + lane * GRAPH_COLUMN_WIDTH}
              y1={0}
              y2="100%"
            />
          ))}
          {incomingLanes.map(lane => {
            const incomingX = GRAPH_NODE_CENTER_X + lane * GRAPH_COLUMN_WIDTH;
            return incomingX === nodeX ? (
              <line
                key={lane}
                data-testid="stack-graph-incoming"
                x1={nodeX}
                x2={nodeX}
                y1={0}
                y2={GRAPH_NODE_CENTER_Y}
              />
            ) : (
              <path
                key={lane}
                data-testid="stack-graph-branch"
                d={`M ${incomingX} 0 C ${incomingX} 6 ${nodeX} 6 ${nodeX} ${GRAPH_ROW_OVERLAP} L ${nodeX} ${GRAPH_NODE_CENTER_Y}`}
                fill="none"
              />
            );
          })}
          {outgoing && (
            <line
              data-testid="stack-graph-outgoing"
              x1={nodeX}
              x2={nodeX}
              y1={GRAPH_NODE_CENTER_Y}
              y2="100%"
            />
          )}
          <circle cx={nodeX} cy={GRAPH_NODE_CENTER_Y} r={GRAPH_NODE_RADIUS} stroke="none" />
        </g>
      </svg>
    </Box>
  );
}

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PullRequestStackItem({
  graphPosition,
  isSelected,
  isDraft,
  number,
  reviewDecision,
  state,
  title,
  totalCommentsCount,
  updatedAt,
}: Props): React.ReactElement {
  const navigateToPullRequest = useNavigateToPullRequest();

  return (
    <ActionList.Item
      data-pull-request-number={number}
      onSelect={() => navigateToPullRequest(number)}
      selected={isSelected}
      sx={{display: 'flex', alignItems: 'center', overflow: 'visible'}}>
      <Box display="flex" alignItems="flex-start" width="100%">
        {graphPosition != null && <StackGraphPrefix {...graphPosition} />}
        <Box fontSize={0} minWidth={0}>
          <Box overflow="hidden" sx={{textOverflow: 'ellipsis'}}>
            <Text fontWeight="bold" fontSize={1} whiteSpace="nowrap">
              {title}
            </Text>
          </Box>
          <BulletItems>
            <PullRequestStateLabel
              isDraft={isDraft}
              reviewDecision={reviewDecision ?? null}
              state={state}
              variant="small"
            />
            <Text>#{number}</Text>
            {formatISODate(updatedAt, false)}
            <CommentCount count={totalCommentsCount ?? 0} />
          </BulletItems>
        </Box>
      </Box>
    </ActionList.Item>
  );
});
