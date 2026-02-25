/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffComment, DiffId} from '../types';

import {Icon} from 'isl-components/Icon';
import {useAtom} from 'jotai';
import {useEffect, useMemo, useState} from 'react';
import {AvatarImg} from '../Avatar';
import {T} from '../i18n';
import {RelativeDate} from '../relativeDate';
import {ReplyInput, ThreadResolutionButton} from '../reviewComments';
import {diffCommentData} from './codeReviewAtoms';

/**
 * Collapsible section showing existing review comments for a specific file.
 * Appears below each file's diff in PR review mode.
 */
export function FileCommentSection({diffId, filePath}: {diffId: DiffId; filePath: string}) {
  const [comments, refresh] = useAtom(diffCommentData(diffId));

  // Fetch comments on mount
  useEffect(() => {
    refresh();
  }, [refresh]);

  const fileComments = useMemo(() => {
    if (comments.state !== 'hasData') {
      return [];
    }
    return comments.data.filter(
      (comment: DiffComment) => comment.filename === filePath,
    );
  }, [comments, filePath]);

  const unresolvedCount = useMemo(
    () => fileComments.filter(c => c.isResolved === false).length,
    [fileComments],
  );

  const [collapsed, setCollapsed] = useState<boolean | null>(null);

  // Default collapsed state: collapsed if all resolved, expanded if any unresolved
  const effectiveCollapsed = collapsed ?? (unresolvedCount === 0);

  if (fileComments.length === 0) {
    return null;
  }

  const commentLabel =
    fileComments.length === 1 ? '1 comment' : `${fileComments.length} comments`;

  return (
    <div className="file-comment-section">
      <div
        className="file-comment-section-header"
        onClick={() => setCollapsed(!effectiveCollapsed)}
        role="button"
        tabIndex={0}
        onKeyDown={e => e.key === 'Enter' && setCollapsed(!effectiveCollapsed)}>
        <Icon icon={effectiveCollapsed ? 'chevron-right' : 'chevron-down'} />
        <Icon icon="comment-discussion" />
        <span>
          {commentLabel}
          {unresolvedCount > 0 && (
            <span className="file-comment-unresolved">
              {' '}
              ({unresolvedCount} unresolved)
            </span>
          )}
        </span>
      </div>
      {!effectiveCollapsed && (
        <div className="file-comment-section-list">
          {fileComments.map((comment, i) => (
            <FileComment key={comment.id ?? i} comment={comment} onRefresh={refresh} />
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * Individual comment rendered in the per-file comment section.
 * Supports reply, resolution, and recursive replies.
 */
function FileComment({
  comment,
  onRefresh,
  isReply,
}: {
  comment: DiffComment;
  onRefresh: () => void;
  isReply?: boolean;
}) {
  const [showReply, setShowReply] = useState(false);
  const [localIsResolved, setLocalIsResolved] = useState(comment.isResolved);
  const isResolved = localIsResolved === true;

  return (
    <div className={`file-comment ${isResolved ? 'file-comment-resolved' : ''}`}>
      <div className="file-comment-header">
        <AvatarImg
          url={comment.authorAvatarUri}
          username={comment.author}
          width={20}
          height={20}
        />
        <b>{comment.authorName ?? comment.author}</b>
        {comment.line != null && !isReply && (
          <span className="file-comment-line">L{comment.line}</span>
        )}
        <RelativeDate date={comment.created} useShortVariant />
        {!isReply && comment.threadId && (
          <ThreadResolutionButton
            threadId={comment.threadId}
            isResolved={localIsResolved ?? false}
            onStatusChange={newStatus => {
              setLocalIsResolved(newStatus);
              onRefresh();
            }}
          />
        )}
      </div>
      <div
        className="file-comment-body rendered-markup"
        dangerouslySetInnerHTML={{__html: comment.html}}
      />
      <div className="file-comment-actions">
        {!isReply && comment.threadId && (
          <button className="file-comment-reply-btn" onClick={() => setShowReply(!showReply)}>
            <Icon icon="comment" />
            <T>Reply</T>
          </button>
        )}
      </div>
      {showReply && comment.threadId && (
        <ReplyInput
          threadId={comment.threadId}
          onCancel={() => setShowReply(false)}
          onSuccess={() => {
            setShowReply(false);
            onRefresh();
          }}
        />
      )}
      {comment.replies.length > 0 && (
        <div className="file-comment-replies">
          {comment.replies.map((reply, i) => (
            <FileComment key={reply.id ?? i} comment={reply} onRefresh={onRefresh} isReply />
          ))}
        </div>
      )}
    </div>
  );
}
