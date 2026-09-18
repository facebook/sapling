/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitHubPullRequestReviewThreadComment} from './github/pullRequestTimelineTypes';

import {ReactionContent} from './generated/graphql';
import {gitHubClientAtom, notificationMessageAtom} from './jotai';
import useRefreshPullRequest from './useRefreshPullRequest';
import {SmileyIcon} from '@primer/octicons-react';
import {ActionList, ActionMenu, Box, Button, IconButton} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {useCallback, useState} from 'react';

const REACTIONS: ReadonlyArray<{content: ReactionContent; emoji: string; label: string}> = [
  {content: ReactionContent.ThumbsUp, emoji: '👍', label: 'Thumbs up'},
  {content: ReactionContent.ThumbsDown, emoji: '👎', label: 'Thumbs down'},
  {content: ReactionContent.Laugh, emoji: '😄', label: 'Laugh'},
  {content: ReactionContent.Hooray, emoji: '🎉', label: 'Hooray'},
  {content: ReactionContent.Confused, emoji: '😕', label: 'Confused'},
  {content: ReactionContent.Heart, emoji: '❤️', label: 'Heart'},
  {content: ReactionContent.Rocket, emoji: '🚀', label: 'Rocket'},
  {content: ReactionContent.Eyes, emoji: '👀', label: 'Eyes'},
];

type ReactionGroup = GitHubPullRequestReviewThreadComment['reactionGroups'][number];

export default function CommentReactions({
  commentID,
  reactionGroups,
}: {
  commentID: string;
  reactionGroups: ReactionGroup[];
}): React.ReactElement {
  const client = useAtomValue(gitHubClientAtom);
  const setNotification = useSetAtom(notificationMessageAtom);
  const refreshPullRequest = useRefreshPullRequest();
  const [busyReaction, setBusyReaction] = useState<ReactionContent | null>(null);

  const toggleReaction = useCallback(
    async (content: ReactionContent) => {
      if (client == null || busyReaction != null) {
        return;
      }
      const existing = reactionGroups.find(group => group.content === content);
      setBusyReaction(content);
      try {
        if (existing?.viewerHasReacted) {
          await client.removeReaction({subjectId: commentID, content});
        } else {
          await client.addReaction({subjectId: commentID, content});
        }
        refreshPullRequest();
      } catch (error) {
        setNotification({
          type: 'error',
          message: `Failed to update reaction: ${
            error instanceof Error ? error.message : String(error)
          }`,
        });
      } finally {
        setBusyReaction(null);
      }
    }, [
      busyReaction,
      client,
      commentID,
      reactionGroups,
      refreshPullRequest,
      setNotification,
    ],
  );

  return (
    <Box display="flex" alignItems="center" gridGap={1} flexWrap="wrap">
      {reactionGroups
        .filter(({count}) => count > 0)
        .map(group => {
          const reaction = REACTIONS.find(({content}) => content === group.content);
          return (
            <Button
              key={group.content}
              aria-label={`${reaction?.label ?? group.content}: ${group.count}`}
              disabled={busyReaction != null}
              onClick={() => toggleReaction(group.content)}
              size="small"
              variant={group.viewerHasReacted ? 'primary' : 'default'}>
              {reaction?.emoji ?? '🙂'} {group.count}
            </Button>
          );
        })}
      <ActionMenu>
        <ActionMenu.Anchor>
          <IconButton
            aria-label="Add reaction"
            icon={SmileyIcon}
            size="small"
            variant="invisible"
            sx={{color: 'fg.muted'}}
          />
        </ActionMenu.Anchor>
        <ActionMenu.Overlay width="auto">
          <ActionList sx={{display: 'grid', gridTemplateColumns: 'repeat(8, 32px)', padding: 1}}>
            {REACTIONS.map(({content, emoji, label}) => (
              <ActionList.Item
                key={content}
                aria-label={label}
                disabled={busyReaction != null}
                onSelect={() => toggleReaction(content)}
                title={label}
                sx={{justifyContent: 'center', padding: 1}}>
                {emoji}
              </ActionList.Item>
            ))}
          </ActionList>
        </ActionMenu.Overlay>
      </ActionMenu>
    </Box>
  );
}
