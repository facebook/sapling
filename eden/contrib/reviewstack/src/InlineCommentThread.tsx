/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  GitHubPullRequestReviewThread,
  GitHubPullRequestReviewThreadComment,
} from './github/pullRequestTimelineTypes';
import type {ID} from './github/types';

import ActorHeading from './ActorHeading';
import CommentLink from './CommentLink';
import CommentReactions from './CommentReactions';
import CommentReply from './CommentReply';
import EditableComment from './EditableComment';
import PendingLabel from './PendingLabel';
import {commentAnchorID} from './commentLinkUtils';
import {PullRequestReviewCommentState} from './generated/graphql';
import {
  gitHubClientAtom,
  gitHubPullRequestJumpToCommentIDAtom,
  notificationMessageAtom,
} from './jotai/atoms';
import useRefreshPullRequest from './useRefreshPullRequest';
import {ChevronDownIcon, ChevronRightIcon} from '@primer/octicons-react';
import {Box, Button, IconButton, Text} from '@primer/react';
import {useAtom, useAtomValue, useSetAtom} from 'jotai';
import {useEffect, useRef, useState} from 'react';

type Props = {
  thread: GitHubPullRequestReviewThread;
};

export default function InlineCommentThread({thread}: Props): React.ReactElement | null {
  const {comments} = thread;
  const lastComment = comments[comments.length - 1];
  const [replyingToID, setReplyingToID] = useState<ID | null>(null);
  const [collapsed, setCollapsed] = useState(thread.isResolved || thread.isHistorical === true);
  const client = useAtomValue(gitHubClientAtom);
  const setNotification = useSetAtom(notificationMessageAtom);
  const refreshPullRequest = useRefreshPullRequest();
  const [resolving, setResolving] = useState(false);

  useEffect(() => {
    if (thread.isResolved || thread.isHistorical) {
      setCollapsed(true);
    }
  }, [thread.isHistorical, thread.isResolved]);

  if (lastComment == null) {
    return null;
  }

  const versionLabel =
    thread.isHistorical && thread.sourceVersionIndex != null
      ? `Comment from V${thread.sourceVersionIndex + 1}`
      : null;

  const toggleResolved = async () => {
    if (client == null || resolving) {
      return;
    }
    setResolving(true);
    try {
      if (thread.isResolved) {
        await client.unresolveReviewThread({threadId: thread.id});
      } else {
        await client.resolveReviewThread({threadId: thread.id});
        setCollapsed(true);
      }
      refreshPullRequest();
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      const message = detail.includes('Resource not accessible by integration')
        ? 'The Aionic ReviewStack GitHub App is not installed for this repository. Install it for the repository, or reconnect with a personal access token that has Pull requests write access.'
        : detail;
      setNotification({
        type: 'error',
        message: `Failed to ${thread.isResolved ? 'unresolve' : 'resolve'} comment: ${message}`,
      });
    } finally {
      setResolving(false);
    }
  };

  return (
    <Box
      backgroundColor="canvas.subtle"
      fontFamily="normal"
      padding={2}
      sx={thread.isHistorical ? {opacity: 0.65, fontStyle: 'italic'} : undefined}>
      <Box
        backgroundColor="canvas.default"
        borderColor="border.default"
        borderWidth={1}
        borderStyle="solid">
        <Box display="flex" alignItems="center" gridGap={1} padding={collapsed ? 1 : 2}>
          <IconButton
            aria-label={collapsed ? 'Expand comment thread' : 'Minimize comment thread'}
            icon={collapsed ? ChevronRightIcon : ChevronDownIcon}
            onClick={() => setCollapsed(value => !value)}
            size="small"
            variant="invisible"
          />
          {collapsed && <ActorHeading actor={comments[0].author} />}
          {versionLabel != null && (
            <Text color="fg.muted" fontSize={0} fontStyle="italic">
              [{versionLabel}]
            </Text>
          )}
          {collapsed && thread.isResolved && (
            <Text color="fg.muted" fontSize={0}>
              Resolved
            </Text>
          )}
        </Box>
        {!collapsed && (
          <>
            {comments.map(comment => (
              <Comment
                key={comment.id}
                comment={comment}
                isReplying={replyingToID === comment.id}
                onReply={() => setReplyingToID(comment.id)}
                onCancelReply={() => setReplyingToID(null)}
              />
            ))}
            {(thread.viewerCanResolve || thread.viewerCanUnresolve) && (
              <Box display="flex" justifyContent="flex-end" padding={2} paddingTop={0}>
                <Button disabled={resolving} onClick={toggleResolved} size="small">
                  {thread.isResolved ? 'Unresolve' : 'Resolve'}
                </Button>
              </Box>
            )}
          </>
        )}
      </Box>
    </Box>
  );
}

function Comment({
  comment,
  isReplying,
  onReply,
  onCancelReply,
}: {
  comment: GitHubPullRequestReviewThreadComment;
  isReplying: boolean;
  onReply: () => void;
  onCancelReply: () => void;
}): React.ReactElement {
  const commitID = comment.originalCommit?.oid ?? comment.commit?.oid;
  const ref = useRef<HTMLDivElement | null>(null);
  const [jumpToCommentID, setJumpToCommentID] = useAtom(
    gitHubPullRequestJumpToCommentIDAtom(comment.id),
  );

  useEffect(() => {
    if (ref.current != null && jumpToCommentID) {
      ref.current.scrollIntoView();
      setJumpToCommentID(false);
    }
  }, [jumpToCommentID, setJumpToCommentID]);

  let pendingLabel = null;
  if (comment.state === PullRequestReviewCommentState.Pending) {
    pendingLabel = <PendingLabel />;
  }

  return (
    <Box id={commentAnchorID(comment.id, 'diff')} ref={ref} padding={2}>
      <Box display="flex" justifyContent="space-between">
        <ActorHeading actor={comment.author} />
        <Box display="flex" alignItems="center" gridGap={1}>
          {pendingLabel}
          <CommentLink id={comment.id} location="diff" />
        </Box>
      </Box>
      <Box fontSize={1} sx={{wordBreak: 'break-word'}}>
        <EditableComment
          id={comment.id}
          authorLogin={comment.author?.login}
          body={comment.body}
          bodyHTML={comment.bodyHTML}
          kind="review"
        />
      </Box>
      <Box marginTop={1}>
        <CommentReactions commentID={comment.id} reactionGroups={comment.reactionGroups} />
      </Box>
      {commitID != null && (
        <CommentReply
          commentID={comment.id}
          commitID={commitID}
          isReplying={isReplying}
          onReply={onReply}
          onCancel={onCancelReply}
        />
      )}
    </Box>
  );
}
