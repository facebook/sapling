/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactionContent} from './generated/graphql';
import type {ID, GitObject} from './github/types';

import CommentLink from './CommentLink';
import CommentReactions from './CommentReactions';
import CommentReply from './CommentReply';
import EditableComment from './EditableComment';
import PullRequestReviewCommentLineNumber from './PullRequestReviewCommentLineNumber';
import {pullRequestTimelineReplyIDAtom} from './PullRequestTimelineReply';
import {commentAnchorID} from './commentLinkUtils';
import {gitHubPullRequestCommentForIDAtom} from './jotai';
import {Box} from '@primer/react';
import {useAtom, useAtomValue} from 'jotai';

type Props = {
  comment: {
    id: ID;
    originalCommit?: GitObject | null;
    commit?: GitObject | null;
    path: string;
    author?: {login: string} | null;
    body: string;
    bodyHTML: string;
    reactionGroups?: Array<{
      content: ReactionContent;
      count?: number;
      reactors?: {totalCount: number};
      viewerHasReacted: boolean;
    }> | null | undefined;
  };
};

export default function PullRequestReviewComment({comment}: Props): React.ReactElement {
  const [replyingToID, setReplyingToID] = useAtom(pullRequestTimelineReplyIDAtom);
  const reviewComment = useAtomValue(gitHubPullRequestCommentForIDAtom(comment.id));
  const commentID = comment.id;
  const commit = comment.originalCommit?.oid ?? comment.commit?.oid;
  const lineNumber = reviewComment?.originalLine;

  return (
    <div className="PRT-review-comment" id={commentAnchorID(comment.id)}>
      <Box color="accent.fg" display="flex" justifyContent="space-between">
        <div className="PRT-review-comment-path-link">{comment.path}</div>
        <CommentLink id={comment.id} />
      </Box>
      <Box display="grid" gridTemplateColumns="25px 1fr">
        <Box textAlign="right">
          {commentID != null && commit != null && lineNumber != null && (
            <PullRequestReviewCommentLineNumber
              commentID={commentID}
              commit={commit}
              lineNumber={lineNumber}
            />
          )}
        </Box>
        <Box paddingLeft={2}>
          <EditableComment
            id={comment.id}
            authorLogin={comment.author?.login}
            body={comment.body}
            kind="review"
            className="PRT-review-comment-text"
            bodyHTML={comment.bodyHTML}
          />
          <CommentReactions
            commentID={comment.id}
            reactionGroups={(comment.reactionGroups ?? []).map(group => ({
              content: group.content,
              count: group.count ?? group.reactors?.totalCount ?? 0,
              viewerHasReacted: group.viewerHasReacted,
            }))}
          />
          {commit != null && (
            <CommentReply
              commentID={commentID}
              commitID={commit}
              isReplying={replyingToID === commentID}
              onReply={() => setReplyingToID(commentID)}
              onCancel={() => setReplyingToID(null)}
            />
          )}
        </Box>
      </Box>
    </div>
  );
}
