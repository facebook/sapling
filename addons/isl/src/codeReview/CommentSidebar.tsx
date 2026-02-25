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
import {T, t} from '../i18n';
import {RelativeDate} from '../relativeDate';
import {ReplyInput, ThreadResolutionButton} from '../reviewComments';
import {commentSidebarOpenAtom, groupCommentsByFile} from './CommentSidebarState';
import {diffCommentData} from './codeReviewAtoms';

import './CommentSidebar.css';

/**
 * Sidebar panel that shows all PR comments grouped by file.
 * Slides in from the right of the ComparisonView when in review mode.
 */
export function CommentSidebar({diffId}: {diffId: DiffId}) {
  const [isOpen, setIsOpen] = useAtom(commentSidebarOpenAtom);
  const [comments, refresh] = useAtom(diffCommentData(diffId));

  // Fetch comments on mount
  useEffect(() => {
    refresh();
  }, [refresh]);

  const grouped = useMemo(
    () => groupCommentsByFile(comments.state === 'hasData' ? comments.data : []),
    [comments],
  );

  // Auto-open when comments exist
  useEffect(() => {
    if (grouped.totalCount > 0) {
      setIsOpen(true);
    }
  }, [grouped.totalCount, setIsOpen]);

  if (!isOpen) {
    return null;
  }

  return (
    <div className="comment-sidebar">
      <div className="comment-sidebar-header">
        <span className="comment-sidebar-title">
          <Icon icon="comment-discussion" />
          <T>Comments</T>
          {grouped.totalCount > 0 && (
            <span className="comment-sidebar-count">{grouped.totalCount}</span>
          )}
        </span>
        <button
          className="comment-sidebar-close"
          onClick={() => setIsOpen(false)}
          title={t('Close comments sidebar')}>
          <Icon icon="x" />
        </button>
      </div>
      <div className="comment-sidebar-content">
        {comments.state === 'loading' ? (
          <div className="comment-sidebar-loading">
            <Icon icon="loading" />
          </div>
        ) : grouped.totalCount === 0 ? (
          <div className="comment-sidebar-empty">
            <Icon icon="comment" />
            <T>No comments yet</T>
          </div>
        ) : (
          <>
            {grouped.general.length > 0 && (
              <SidebarFileGroup
                title={t('General')}
                comments={grouped.general}
                onRefresh={refresh}
              />
            )}
            {[...grouped.byFile.entries()].map(([filePath, fileComments]) => (
              <SidebarFileGroup
                key={filePath}
                title={filePath}
                comments={fileComments}
                isFilePath
                onRefresh={refresh}
              />
            ))}
          </>
        )}
      </div>
    </div>
  );
}

/**
 * Collapsible group of comments for a file (or general comments).
 */
function SidebarFileGroup({
  title,
  comments,
  isFilePath,
  onRefresh,
}: {
  title: string;
  comments: DiffComment[];
  isFilePath?: boolean;
  onRefresh: () => void;
}) {
  const [collapsed, setCollapsed] = useState(false);
  const unresolvedCount = comments.filter(c => c.isResolved === false).length;

  return (
    <div className="sidebar-file-group">
      <div
        className="sidebar-file-group-header"
        onClick={() => setCollapsed(!collapsed)}
        role="button"
        tabIndex={0}
        onKeyDown={e => e.key === 'Enter' && setCollapsed(!collapsed)}>
        <Icon icon={collapsed ? 'chevron-right' : 'chevron-down'} />
        <span className="sidebar-file-group-title" title={isFilePath ? title : undefined}>
          {isFilePath ? title.split('/').pop() ?? title : title}
        </span>
        <span className="sidebar-file-group-count">
          {comments.length}
          {unresolvedCount > 0 && (
            <span className="sidebar-unresolved-count"> ({unresolvedCount} open)</span>
          )}
        </span>
      </div>
      {!collapsed &&
        comments.map((comment, i) => (
          <SidebarComment key={comment.id ?? i} comment={comment} onRefresh={onRefresh} />
        ))}
    </div>
  );
}

/**
 * Individual comment rendered in the sidebar.
 */
function SidebarComment({
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
    <div className={`sidebar-comment ${isResolved ? 'sidebar-comment-resolved' : ''}`}>
      {comment.line != null && !isReply && (
        <span className="sidebar-comment-location">
          L{comment.line}
        </span>
      )}
      <div className="sidebar-comment-header">
        <AvatarImg
          url={comment.authorAvatarUri}
          username={comment.author}
          width={18}
          height={18}
        />
        <b className="sidebar-comment-author">{comment.authorName ?? comment.author}</b>
        <RelativeDate date={comment.created} useShortVariant />
      </div>
      <div
        className="sidebar-comment-body rendered-markup"
        dangerouslySetInnerHTML={{__html: comment.html}}
      />
      <div className="sidebar-comment-actions">
        {!isReply && comment.threadId && (
          <>
            <button className="sidebar-reply-btn" onClick={() => setShowReply(!showReply)}>
              <Icon icon="comment" />
              <T>Reply</T>
            </button>
            <ThreadResolutionButton
              threadId={comment.threadId}
              isResolved={localIsResolved ?? false}
              onStatusChange={newStatus => {
                setLocalIsResolved(newStatus);
                onRefresh();
              }}
            />
          </>
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
        <div className="sidebar-comment-replies">
          {comment.replies.map((reply, i) => (
            <SidebarComment key={reply.id ?? i} comment={reply} onRefresh={onRefresh} isReply />
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * Toggle button for the comment sidebar, placed in the ComparisonView header.
 */
export function CommentSidebarToggle({commentCount}: {commentCount: number}) {
  const [isOpen, setIsOpen] = useAtom(commentSidebarOpenAtom);

  return (
    <button
      className={`comment-sidebar-toggle ${isOpen ? 'comment-sidebar-toggle-active' : ''}`}
      onClick={() => setIsOpen(!isOpen)}
      title={t('Toggle comments sidebar')}>
      <Icon icon="comment-discussion" />
      {commentCount > 0 && <span className="comment-toggle-count">{commentCount}</span>}
    </button>
  );
}
