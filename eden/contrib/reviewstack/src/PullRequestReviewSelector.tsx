/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useCommand} from './KeyboardShortcuts';
import {PullRequestReviewEvent} from './generated/graphql';
import {gitHubPullRequestViewerDidAuthor} from './recoil';
import {ActionList, ActionMenu} from '@primer/react';
import React from 'react';
import {useRecoilValue} from 'recoil';
import {isMac} from 'shared/OperatingSystem';

const MODIFIER = isMac ? '\u2325' : 'Alt';

const ACTION = {
  Approve: {
    event: PullRequestReviewEvent.Approve,
    shortcut: `${MODIFIER} A`,
  },
  Comment: {
    event: PullRequestReviewEvent.Comment,
    shortcut: `${MODIFIER} C`,
  },
  RequestChanges: {
    event: PullRequestReviewEvent.RequestChanges,
    shortcut: `${MODIFIER} R`,
  },
};

const AUTHOR_ACTIONS = [ACTION.Comment];

const REVIEWER_ACTIONS = [ACTION.Comment, ACTION.Approve, ACTION.RequestChanges];

type Props = {
  event: PullRequestReviewEvent;
  onSelect: (event: PullRequestReviewEvent) => void;
};

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PullRequestReviewSelector({
  event,
  onSelect,
}: Props): React.ReactElement {
  const viewerDidAuthor = useRecoilValue(gitHubPullRequestViewerDidAuthor);
  const actions = viewerDidAuthor ? AUTHOR_ACTIONS : REVIEWER_ACTIONS;

  useCommand('Comment', () => {
    onSelect(PullRequestReviewEvent.Comment);
  });
  useCommand('Approve', () => {
    if (!viewerDidAuthor) {
      onSelect(PullRequestReviewEvent.Approve);
    }
  });
  useCommand('RequestChanges', () => {
    if (!viewerDidAuthor) {
      onSelect(PullRequestReviewEvent.RequestChanges);
    }
  });

  return (
    <ActionMenu>
      <ActionMenu.Button>{eventLabel(event)}</ActionMenu.Button>
      <ActionMenu.Overlay width="medium">
        <ActionList selectionVariant="single">
          {actions.map(action => (
            <ActionList.Item
              key={action.event}
              onSelect={() => onSelect(action.event)}
              selected={event === action.event}>
              {eventLabel(action.event)}
              <ActionList.TrailingVisual>{action.shortcut}</ActionList.TrailingVisual>
            </ActionList.Item>
          ))}
        </ActionList>
      </ActionMenu.Overlay>
    </ActionMenu>
  );
});

function eventLabel(event: PullRequestReviewEvent): string {
  switch (event) {
    case PullRequestReviewEvent.Approve:
      return 'Approve';
    case PullRequestReviewEvent.Comment:
      return 'Comment';
    case PullRequestReviewEvent.Dismiss:
      return 'Dismiss';
    case PullRequestReviewEvent.RequestChanges:
      return 'Request Changes';
  }
}
