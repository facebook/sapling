/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ParsedDiff} from 'shared/patch/types';
import type {SidebarTab} from './CommentSidebarState';
import type {DiffComment, DiffId} from '../types';

import {Icon} from 'isl-components/Icon';
import {useAtom, useAtomValue} from 'jotai';
import {useEffect, useMemo, useState} from 'react';
import {DiffType} from 'shared/patch/types';
import {AvatarImg} from '../Avatar';
import {T, t} from '../i18n';
import {RelativeDate} from '../relativeDate';
import serverAPI from '../ClientToServerAPI';
import {currentGitHubUser} from './CodeReviewInfo';
import {ReplyInput, ThreadResolutionButton} from '../reviewComments';
import {commentSidebarOpenAtom, groupCommentsByFile, sidebarActiveTabAtom} from './CommentSidebarState';
import {diffCommentData} from './codeReviewAtoms';

import './CommentSidebar.css';

type FileStats = {
  path: string;
  additions: number;
  deletions: number;
  fileType: 'added' | 'removed' | 'modified' | 'renamed';
};

function computeFileStats(diffs: ParsedDiff[]): FileStats[] {
  return diffs.map(diff => {
    let additions = 0;
    let deletions = 0;
    for (const hunk of diff.hunks) {
      for (const line of hunk.lines) {
        if (line.startsWith('+')) {
          additions++;
        } else if (line.startsWith('-')) {
          deletions++;
        }
      }
    }

    let fileType: FileStats['fileType'];
    if (diff.type === DiffType.Added) {
      fileType = 'added';
    } else if (diff.type === DiffType.Removed || diff.newFileName === '/dev/null') {
      fileType = 'removed';
    } else if (
      diff.type === DiffType.Renamed ||
      (diff.oldFileName !== diff.newFileName &&
        diff.oldFileName != null &&
        diff.newFileName != null)
    ) {
      fileType = 'renamed';
    } else {
      fileType = 'modified';
    }

    const path = diff.newFileName ?? diff.oldFileName ?? '';
    return {path, additions, deletions, fileType};
  });
}

/** Truncate a file path to show ...last/three/segments */
function truncatePath(path: string, maxSegments = 3): string {
  const parts = path.split('/');
  if (parts.length <= maxSegments) {
    return path;
  }
  return '\u2026/' + parts.slice(-maxSegments).join('/');
}

const fileTypeToIcon: Record<FileStats['fileType'], string> = {
  added: 'diff-added',
  removed: 'diff-removed',
  modified: 'diff-modified',
  renamed: 'diff-renamed',
};

/**
 * Sidebar panel that shows PR comments and file list in tabs.
 * Slides in from the right of the ComparisonView when in review mode.
 */
export function CommentSidebar({
  diffId,
  diffs,
  onFileClick,
  nodeId,
}: {
  diffId: DiffId;
  diffs?: ParsedDiff[];
  onFileClick?: (path: string) => void;
  nodeId?: string;
}) {
  const [isOpen, setIsOpen] = useAtom(commentSidebarOpenAtom);
  const [activeTab, setActiveTab] = useAtom(sidebarActiveTabAtom);
  const [comments, refresh] = useAtom(diffCommentData(diffId));
  const currentUser = useAtomValue(currentGitHubUser);

  // Fetch comments on mount
  useEffect(() => {
    refresh();
  }, [refresh]);

  // Keep last known comments while refreshing to avoid flashing spinner
  const [lastComments, setLastComments] = useState<import('../types').DiffComment[]>([]);
  useEffect(() => {
    if (comments.state === 'hasData') {
      setLastComments(comments.data);
    }
  }, [comments]);
  const grouped = useMemo(
    () => groupCommentsByFile(comments.state === 'hasData' ? comments.data : lastComments),
    [comments, lastComments],
  );
  const isInitialLoad = comments.state === 'loading' && lastComments.length === 0;

  const fileStats = useMemo(() => (diffs ? computeFileStats(diffs) : []), [diffs]);

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
        <div className="comment-sidebar-tabs">
          <button
            className={`sidebar-tab ${activeTab === 'files' ? 'sidebar-tab-active' : ''}`}
            onClick={() => setActiveTab('files')}>
            <Icon icon="files" />
            <T>Files</T>
          </button>
          <button
            className={`sidebar-tab ${activeTab === 'comments' ? 'sidebar-tab-active' : ''}`}
            onClick={() => setActiveTab('comments')}>
            <Icon icon="comment-discussion" />
            <T>Comments</T>
            {grouped.totalCount > 0 && (
              <span className="sidebar-tab-badge">{grouped.totalCount}</span>
            )}
          </button>
        </div>
        <button
          className="comment-sidebar-close"
          onClick={() => setIsOpen(false)}
          title={t('Close sidebar')}>
          <Icon icon="x" />
        </button>
      </div>
      <div className="comment-sidebar-content">
        {activeTab === 'comments' ? (
          <CommentsTabContent grouped={grouped} isInitialLoad={isInitialLoad} refresh={refresh} nodeId={nodeId} currentUser={currentUser} />
        ) : (
          <FilesTabContent fileStats={fileStats} onFileClick={onFileClick} />
        )}
      </div>
    </div>
  );
}

function CommentsTabContent({
  grouped,
  isInitialLoad,
  refresh,
  nodeId,
  currentUser,
}: {
  grouped: ReturnType<typeof groupCommentsByFile>;
  isInitialLoad: boolean;
  refresh: () => void;
  nodeId?: string;
  currentUser?: string;
}) {
  const [showNewComment, setShowNewComment] = useState(false);
  const [newCommentBody, setNewCommentBody] = useState('');
  const [submitting, setSubmitting] = useState(false);

  if (isInitialLoad) {
    return (
      <div className="comment-sidebar-loading">
        <Icon icon="loading" />
      </div>
    );
  }

  const handleSubmitNewComment = () => {
    if (newCommentBody.trim() === '' || !nodeId) {
      return;
    }
    setSubmitting(true);
    serverAPI.postMessage({
      type: 'graphqlAddComment',
      subjectId: nodeId,
      body: newCommentBody.trim(),
    });
    const disposable = serverAPI.onMessageOfType('graphqlAddCommentResult', result => {
      disposable.dispose();
      setSubmitting(false);
      if (result.success) {
        setNewCommentBody('');
        setShowNewComment(false);
        refresh();
      }
    });
  };

  return (
    <>
      {grouped.totalCount === 0 && !showNewComment ? (
        <div className="comment-sidebar-empty">
          <Icon icon="comment" />
          <T>No comments yet</T>
        </div>
      ) : (
        <>
          {grouped.general.length > 0 && (
            <SidebarFileGroup title={t('General')} comments={grouped.general} onRefresh={refresh} currentUser={currentUser} nodeId={nodeId} />
          )}
          {[...grouped.byFile.entries()].map(([filePath, fileComments]) => (
            <SidebarFileGroup
              key={filePath}
              title={filePath}
              comments={fileComments}
              isFilePath
              onRefresh={refresh}
              currentUser={currentUser}
              nodeId={nodeId}
            />
          ))}
        </>
      )}
      {/* New comment at bottom */}
      <div className="sidebar-new-comment-row">
        {showNewComment ? (
          <div className="sidebar-new-comment-input">
            <textarea
              className="sidebar-new-comment-textarea"
              value={newCommentBody}
              onChange={e => setNewCommentBody(e.target.value)}
              onKeyDown={e => {
                if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                  e.preventDefault();
                  handleSubmitNewComment();
                }
                if (e.key === 'Escape') {
                  setShowNewComment(false);
                  setNewCommentBody('');
                }
              }}
              placeholder="Write a review comment..."
              autoFocus
            />
            <div className="sidebar-new-comment-actions">
              <span className="sidebar-new-comment-hint">{'\u2318'}Enter to submit</span>
              <button
                className="sidebar-new-comment-cancel"
                onClick={() => { setShowNewComment(false); setNewCommentBody(''); }}>
                <T>Cancel</T>
              </button>
              <button
                className="sidebar-new-comment-submit"
                disabled={newCommentBody.trim() === '' || submitting || !nodeId}
                onClick={handleSubmitNewComment}>
                {submitting ? <Icon icon="loading" /> : <T>Comment</T>}
              </button>
            </div>
          </div>
        ) : (
          <button
            className="sidebar-new-comment-btn"
            onClick={() => setShowNewComment(true)}>
            <Icon icon="add" />
            <T>New comment</T>
          </button>
        )}
      </div>
    </>
  );
}

/** A node in the file tree — either a directory or a file leaf. */
type FileTreeNode = {
  name: string;
  /** Full path from root — only set on file leaves. */
  fullPath?: string;
  fileStats?: FileStats;
  children: Map<string, FileTreeNode>;
};

/** Build a tree from a flat list of file paths. */
function buildFileTree(files: FileStats[]): FileTreeNode {
  const root: FileTreeNode = {name: '', children: new Map()};
  for (const file of files) {
    const parts = file.path.split('/');
    let node = root;
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      if (!node.children.has(part)) {
        node.children.set(part, {name: part, children: new Map()});
      }
      node = node.children.get(part)!;
      if (i === parts.length - 1) {
        // leaf
        node.fullPath = file.path;
        node.fileStats = file;
      }
    }
  }
  return root;
}


function FilesTabContent({
  fileStats,
  onFileClick,
}: {
  fileStats: FileStats[];
  onFileClick?: (path: string) => void;
}) {
  const tree = useMemo(() => buildFileTree(fileStats), [fileStats]);

  if (fileStats.length === 0) {
    return (
      <div className="comment-sidebar-empty">
        <Icon icon="file" />
        <T>No files</T>
      </div>
    );
  }

  const totalAdditions = fileStats.reduce((sum, f) => sum + f.additions, 0);
  const totalDeletions = fileStats.reduce((sum, f) => sum + f.deletions, 0);

  return (
    <div className="sidebar-files-list">
      <div className="sidebar-files-summary">
        {fileStats.length} {fileStats.length === 1 ? 'file' : 'files'}
        {totalAdditions > 0 && <span className="file-stat-add"> +{totalAdditions}</span>}
        {totalDeletions > 0 && <span className="file-stat-del"> &minus;{totalDeletions}</span>}
      </div>
      <div className="file-tree">
        {/* Render root's children (root itself is unnamed) */}
        {[...tree.children.values()].map(child => (
          <FileTreeNodeView key={child.name} node={child} depth={0} onFileClick={onFileClick} />
        ))}
      </div>
    </div>
  );
}

function FileTreeNodeView({
  node,
  depth,
  onFileClick,
}: {
  node: FileTreeNode;
  depth: number;
  onFileClick?: (path: string) => void;
}) {
  const [collapsed, setCollapsed] = useState(false);
  const isFile = node.fullPath != null;
  const indent = depth * 12;

  if (isFile) {
    const file = node.fileStats!;
    return (
      <button
        className="file-tree-file"
        style={{paddingLeft: indent + 8}}
        onClick={() => onFileClick?.(file.path)}
        title={file.path}>
        <Icon icon={fileTypeToIcon[file.fileType]} className={`sidebar-file-icon-${file.fileType}`} size="XS" />
        <span className="file-tree-name">{node.name}</span>
        <span className="sidebar-file-stats">
          {file.additions > 0 && <span className="file-stat-add">+{file.additions}</span>}
          {file.deletions > 0 && <span className="file-stat-del">&minus;{file.deletions}</span>}
        </span>
      </button>
    );
  }

  // Directory node
  const children = [...node.children.values()];
  return (
    <div className="file-tree-dir">
      <button
        className="file-tree-dir-toggle"
        style={{paddingLeft: indent + 4}}
        onClick={() => setCollapsed(c => !c)}>
        <Icon icon={collapsed ? 'chevron-right' : 'chevron-down'} size="XS" />
        <Icon icon="folder" size="XS" className="file-tree-folder-icon" />
        <span className="file-tree-name">{node.name}</span>
      </button>
      {!collapsed && children.map(child => (
        <FileTreeNodeView key={child.name} node={child} depth={depth + 1} onFileClick={onFileClick} />
      ))}
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
  currentUser,
  nodeId,
}: {
  title: string;
  comments: DiffComment[];
  isFilePath?: boolean;
  onRefresh: () => void;
  currentUser?: string;
  nodeId?: string;
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
          {isFilePath ? truncatePath(title) : title}
        </span>
        <span className="sidebar-file-group-count">
          {comments.length}
          {unresolvedCount > 0 && (
            <span className="sidebar-unresolved-count"> ({unresolvedCount} open)</span>
          )}
        </span>
      </div>
      {!collapsed && (
        <ThreadedComments comments={comments} onRefresh={onRefresh} currentUser={currentUser} nodeId={nodeId} />
      )}
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
  currentUser,
  nodeId,
}: {
  comment: DiffComment;
  onRefresh: () => void;
  isReply?: boolean;
  currentUser?: string;
  nodeId?: string;
}) {
  const [showReply, setShowReply] = useState(false);
  const [localIsResolved, setLocalIsResolved] = useState(comment.isResolved);
  const isResolved = localIsResolved === true;
  const [editing, setEditing] = useState(false);
  const [editBody, setEditBody] = useState(comment.content ?? '');
  const [editSubmitting, setEditSubmitting] = useState(false);

  const isOwnComment = currentUser != null && comment.author === currentUser;
  const canEdit = isOwnComment && comment.id != null;

  const handleEdit = () => {
    if (!comment.id || editBody.trim() === '') {
      return;
    }
    setEditSubmitting(true);
    serverAPI.postMessage({
      type: 'graphqlEditComment',
      commentId: comment.id,
      body: editBody.trim(),
    });
    const disposable = serverAPI.onMessageOfType('graphqlEditCommentResult', result => {
      disposable.dispose();
      setEditSubmitting(false);
      if (result.success) {
        setEditing(false);
        onRefresh();
      }
    });
  };

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
        {/* Edit icon — hover-revealed in header for own comments */}
        {canEdit && !editing && (
          <button
            className="sidebar-comment-edit-icon"
            onClick={() => setEditing(true)}
            title={t('Edit comment')}>
            <Icon icon="edit" size="XS" />
          </button>
        )}
      </div>
      {editing ? (
        <div className="sidebar-edit-container">
          <textarea
            className="sidebar-new-comment-textarea"
            value={editBody}
            onChange={e => setEditBody(e.target.value)}
            onKeyDown={e => {
              if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                e.preventDefault();
                handleEdit();
              }
              if (e.key === 'Escape') {
                setEditing(false);
                setEditBody(comment.content ?? '');
              }
            }}
            autoFocus
          />
          <div className="sidebar-new-comment-actions">
            <span className="sidebar-new-comment-hint">{'\u2318'}Enter to save</span>
            <button
              className="sidebar-new-comment-cancel"
              onClick={() => { setEditing(false); setEditBody(comment.content ?? ''); }}>
              <T>Cancel</T>
            </button>
            <button
              className="sidebar-new-comment-submit"
              disabled={editBody.trim() === '' || editSubmitting}
              onClick={handleEdit}>
              {editSubmitting ? <Icon icon="loading" /> : <T>Save</T>}
            </button>
          </div>
        </div>
      ) : (
        <div
          className="sidebar-comment-body rendered-markup"
          dangerouslySetInnerHTML={{__html: comment.html}}
        />
      )}
      {/* Actions row — only shown when there are thread actions */}
      {!isReply && comment.threadId && (
        <div className="sidebar-comment-actions">
          <button className="sidebar-action-btn" onClick={() => setShowReply(!showReply)}>
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
        </div>
      )}
      {/* Reply input for review threads */}
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
    </div>
  );
}

/**
 * Groups comments by threadId and renders each thread as a collapsible conversation.
 * Single comments or comments without threadId render standalone.
 */
function ThreadedComments({
  comments,
  onRefresh,
  currentUser,
  nodeId,
}: {
  comments: DiffComment[];
  onRefresh: () => void;
  currentUser?: string;
  nodeId?: string;
}) {
  // Group comments by threadId; comments without threadId are standalone
  const threads = useMemo(() => {
    const threadMap = new Map<string, DiffComment[]>();
    const standalone: DiffComment[] = [];

    for (const comment of comments) {
      if (comment.threadId) {
        const existing = threadMap.get(comment.threadId) ?? [];
        existing.push(comment);
        threadMap.set(comment.threadId, existing);
      } else {
        standalone.push(comment);
      }
    }

    const result: Array<{key: string; comments: DiffComment[]; isThread: boolean}> = [];
    for (const s of standalone) {
      result.push({key: s.id ?? String(result.length), comments: [s], isThread: false});
    }
    for (const [threadId, threadComments] of threadMap) {
      result.push({key: threadId, comments: threadComments, isThread: threadComments.length > 1});
    }
    return result;
  }, [comments]);

  return (
    <>
      {threads.map(thread =>
        thread.isThread ? (
          <CollapsibleThread key={thread.key} comments={thread.comments} onRefresh={onRefresh} currentUser={currentUser} nodeId={nodeId} />
        ) : (
          <SidebarComment key={thread.key} comment={thread.comments[0]} onRefresh={onRefresh} currentUser={currentUser} nodeId={nodeId} />
        ),
      )}
    </>
  );
}

/**
 * A collapsible thread: shows the first comment, with a toggle to show/hide the rest.
 */
function CollapsibleThread({
  comments,
  onRefresh,
  currentUser,
  nodeId,
}: {
  comments: DiffComment[];
  onRefresh: () => void;
  currentUser?: string;
  nodeId?: string;
}) {
  const [collapsed, setCollapsed] = useState(false);
  const [first, ...rest] = comments;

  return (
    <div className="sidebar-thread">
      <SidebarComment comment={first} onRefresh={onRefresh} currentUser={currentUser} nodeId={nodeId} />
      {rest.length > 0 && (
        <div className="sidebar-comment-replies">
          <button
            className="sidebar-replies-toggle"
            onClick={() => setCollapsed(!collapsed)}>
            <Icon icon={collapsed ? 'chevron-right' : 'chevron-down'} />
            {collapsed
              ? t('Show $count replies', {replace: {$count: String(rest.length)}})
              : t('$count replies', {replace: {$count: String(rest.length)}})}
          </button>
          {!collapsed &&
            rest.map((reply, i) => (
              <SidebarComment key={reply.id ?? i} comment={reply} onRefresh={onRefresh} isReply currentUser={currentUser} nodeId={nodeId} />
            ))}
        </div>
      )}
    </div>
  );
}

/**
 * Toggle buttons for the sidebar, placed in the ComparisonView header.
 * Comments button opens to comments tab, Files button opens to files tab.
 */
export function SidebarToggleButtons({commentCount}: {commentCount: number}) {
  const [isOpen, setIsOpen] = useAtom(commentSidebarOpenAtom);
  const [activeTab, setActiveTab] = useAtom(sidebarActiveTabAtom);

  const handleToggle = (tab: SidebarTab) => {
    if (isOpen && activeTab === tab) {
      setIsOpen(false);
    } else {
      setActiveTab(tab);
      setIsOpen(true);
    }
  };

  return (
    <div className="sidebar-toggle-buttons">
      <button
        className={`comment-sidebar-toggle ${isOpen && activeTab === 'files' ? 'comment-sidebar-toggle-active' : ''}`}
        onClick={() => handleToggle('files')}
        title={t('Toggle file list')}>
        <Icon icon="files" />
      </button>
      <button
        className={`comment-sidebar-toggle ${isOpen && activeTab === 'comments' ? 'comment-sidebar-toggle-active' : ''}`}
        onClick={() => handleToggle('comments')}
        title={t('Toggle comments')}>
        <span className="toggle-icon-wrapper">
          <Icon icon="comment-discussion" />
          {commentCount > 0 && <span className="toggle-badge">{commentCount}</span>}
        </span>
      </button>
    </div>
  );
}
