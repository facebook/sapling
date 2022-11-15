/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ID, GitObject} from './github/types';

import PullRequestReviewCommentLineNumber from './PullRequestReviewCommentLineNumber';
import TrustedRenderedMarkdown from './TrustedRenderedMarkdown';
import {gitHubPullRequestCommentForID} from './recoil';
import {Box} from '@primer/react';
import {useRecoilValue} from 'recoil';

type Props = {
  comment: {
    id: ID;
    originalCommit?: GitObject | null;
    path: string;
    bodyHTML: string;
  };
};

export default function PullRequestReviewComment({comment}: Props): React.ReactElement {
  const reviewComment = useRecoilValue(gitHubPullRequestCommentForID(comment.id));
  const commentID = comment.id;
  const commit = comment.originalCommit?.oid;
  const lineNumber = reviewComment?.originalLine;

  return (
    <div className="PRT-review-comment">
      <Box color="accent.fg">
        <div className="PRT-review-comment-path-link">{comment.path}</div>
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
          <TrustedRenderedMarkdown
            className="PRT-review-comment-text"
            trustedHTML={comment.bodyHTML}
          />
        </Box>
      </Box>
    </div>
  );
}
