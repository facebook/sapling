/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitHubPullRequestReviewThreadComment} from './github/pullRequestTimelineTypes';
import type {GitObjectID, ID} from './github/types';

import ActorHeading from './ActorHeading';
import CommentLink from './CommentLink';
import EditableComment from './EditableComment';
import PendingLabel from './PendingLabel';
import PullRequestInlineCommentInput from './PullRequestInlineCommentInput';
import {commentAnchorID} from './commentLinkUtils';
import {PullRequestReviewCommentState} from './generated/graphql';
import {gitHubPullRequestJumpToCommentIDAtom} from './jotai/atoms';
import {Box, Button} from '@primer/react';
import {useAtom} from 'jotai';
import {useEffect, useRef, useState} from 'react';

type Props = {
  comments: GitHubPullRequestReviewThreadComment[];
};

export default function InlineCommentThread({comments}: Props): React.ReactElement | null {
  const lastComment = comments[comments.length - 1];
  if (lastComment == null) {
    return null;
  }

  const commentID = lastComment.id;
  const commitID = lastComment.originalCommit?.oid;

  let reply = null;
  if (commitID != null) {
    reply = <Reply commentID={commentID} commitID={commitID} />;
  }

  return (
    <Box backgroundColor="canvas.subtle" fontFamily="normal" padding={2}>
      <Box
        backgroundColor="canvas.default"
        borderColor="border.default"
        borderWidth={1}
        borderStyle="solid">
        {comments.map((comment, index) => (
          <Comment key={index} comment={comment} />
        ))}
        {reply}
      </Box>
    </Box>
  );
}

function Comment({comment}: {comment: GitHubPullRequestReviewThreadComment}): React.ReactElement {
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
    </Box>
  );
}

function Reply(props: {commentID: ID; commitID: GitObjectID}): React.ReactElement {
  const [showReply, setShowReply] = useState(false);

  if (showReply) {
    return <PullRequestInlineCommentInput {...props} onCancel={() => setShowReply(false)} />;
  }

  return (
    <Box
      display="flex"
      justifyContent="flex-end"
      backgroundColor="canvas.subtle"
      borderTopColor="border.default"
      borderTopWidth={1}
      borderTopStyle="solid"
      padding={2}>
      <Button onClick={() => setShowReply(true)}>Reply</Button>
    </Box>
  );
}
