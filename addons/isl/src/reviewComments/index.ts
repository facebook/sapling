/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export {
  type PendingComment,
  pendingCommentsAtom,
  addPendingComment,
  removePendingComment,
  clearPendingComments,
  getPendingCommentCount,
} from './pendingCommentsState';

export {CommentInput, type CommentInputProps} from './CommentInput';

export {PendingCommentDisplay, type PendingCommentDisplayProps} from './PendingCommentDisplay';

export {PendingCommentsBadge, type PendingCommentsBadgeProps} from './PendingCommentsBadge';

export {ReplyInput, type ReplyInputProps} from './ReplyInput';

export {
  ThreadResolutionButton,
  type ThreadResolutionButtonProps,
} from './ThreadResolution';

export {
  getSyncWarnings,
  formatSyncWarningMessage,
  type SyncWarnings,
} from './syncWarning';
