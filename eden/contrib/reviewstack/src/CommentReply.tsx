/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitObjectID, ID} from './github/types';

import PullRequestInlineCommentInput from './PullRequestInlineCommentInput';
import {ReplyIcon} from '@primer/octicons-react';
import {Box, IconButton} from '@primer/react';

type Props = {
  commentID: ID;
  commitID: GitObjectID;
  isReplying: boolean;
  onReply: () => void;
  onCancel: () => void;
};

export default function CommentReply({
  commentID,
  commitID,
  isReplying,
  onReply,
  onCancel,
}: Props): React.ReactElement {
  if (isReplying) {
    return (
      <PullRequestInlineCommentInput
        commentID={commentID}
        commitID={commitID}
        onCancel={onCancel}
      />
    );
  }

  return (
    <Box display="flex" justifyContent="flex-end" marginTop={1}>
      <IconButton
        aria-label="Reply to comment"
        icon={ReplyIcon}
        onClick={onReply}
        size="small"
        variant="invisible"
      />
    </Box>
  );
}
