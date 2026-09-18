/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PullRequestReviewDecision, PullRequestState} from './generated/graphql';

import PullRequestStateLabel from './PullRequestStateLabel';
import {PullRequestState as PullRequestStateValue} from './generated/graphql';
import {gitHubClientAtom, notificationMessageAtom} from './jotai';
import pullRequestStatusAndLabel from './pullRequestStatusAndLabel';
import useRefreshPullRequest from './useRefreshPullRequest';
import {ActionList, ActionMenu, Button, StateLabel} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {useCallback, useState} from 'react';

export default function PullRequestDraftStateMenu({
  id,
  isDraft,
  reviewDecision,
  state,
  viewerCanUpdate,
}: {
  id: string;
  isDraft: boolean;
  reviewDecision: PullRequestReviewDecision | null;
  state: PullRequestState;
  viewerCanUpdate: boolean;
}): React.ReactElement {
  const client = useAtomValue(gitHubClientAtom);
  const refreshPullRequest = useRefreshPullRequest();
  const setNotification = useSetAtom(notificationMessageAtom);
  const [updating, setUpdating] = useState(false);

  const updateDraftState = useCallback(
    async (nextIsDraft: boolean) => {
      if (client == null || nextIsDraft === isDraft) {
        return;
      }
      setUpdating(true);
      try {
        if (nextIsDraft) {
          await client.convertPullRequestToDraft({pullRequestId: id});
        } else {
          await client.markPullRequestReadyForReview({pullRequestId: id});
        }
        refreshPullRequest();
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        setNotification({
          type: 'error',
          message: `Failed to update pull request state: ${message}`,
        });
      } finally {
        setUpdating(false);
      }
    },
    [client, id, isDraft, refreshPullRequest, setNotification],
  );

  if (state !== PullRequestStateValue.Open || !viewerCanUpdate) {
    return (
      <PullRequestStateLabel isDraft={isDraft} reviewDecision={reviewDecision} state={state} />
    );
  }

  const {label, color} = pullRequestStatusAndLabel(state, reviewDecision, isDraft);
  return (
    <ActionMenu>
      <ActionMenu.Anchor>
        <Button
          aria-label={`${label}. Change pull request state`}
          disabled={updating}
          variant="invisible"
          sx={{height: 'auto', padding: 0}}>
          <StateLabel
            status="pullOpened"
            sx={{backgroundColor: color, cursor: updating ? 'wait' : 'pointer'}}>
            {label}
          </StateLabel>
        </Button>
      </ActionMenu.Anchor>
      <ActionMenu.Overlay width="small">
        <ActionList selectionVariant="single">
          <ActionList.Item
            selected={isDraft}
            disabled={updating || isDraft}
            onSelect={() => updateDraftState(true)}>
            Convert to draft
          </ActionList.Item>
          <ActionList.Item
            selected={!isDraft}
            disabled={updating || !isDraft}
            onSelect={() => updateDraftState(false)}>
            Mark ready for review
          </ActionList.Item>
        </ActionList>
      </ActionMenu.Overlay>
    </ActionMenu>
  );
}
