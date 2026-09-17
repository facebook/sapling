/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitHubPullRequestReviewThreadComment} from './github/pullRequestTimelineTypes';
import type {ID} from './github/types';

import ActorHeading from './ActorHeading';
import CommentLink from './CommentLink';
import CommentReply from './CommentReply';
import EditableComment from './EditableComment';
import PendingLabel from './PendingLabel';
import {commentAnchorID} from './commentLinkUtils';
import {PullRequestReviewCommentState} from './generated/graphql';
import {gitHubPullRequestJumpToCommentIDAtom} from './jotai/atoms';
import {Box} from '@primer/react';
import {useAtom} from 'jotai';
import {useEffect, useRef, useState} from 'react';

type Props = {
  comments: GitHubPullRequestReviewThreadComment[];
};

export default function InlineCommentThread({comments}: Props): React.ReactElement | null {
  const lastComment = comments[comments.length - 1];
  const [replyingToID, setReplyingToID] = useState<ID | null>(null);
  if (lastComment == null) {
    return null;
  }

  return (
    <Box backgroundColor="canvas.subtle" fontFamily="normal" padding={2}>
      <Box
        backgroundColor="canvas.default"
        borderColor="border.default"
        borderWidth={1}
        borderStyle="solid">
        {comments.map(comment => (
          <Comment
            key={comment.id}
            comment={comment}
            isReplying={replyingToID === comment.id}
            onReply={() => setReplyingToID(comment.id)}
            onCancelReply={() => setReplyingToID(null)}
          />
        ))}
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
