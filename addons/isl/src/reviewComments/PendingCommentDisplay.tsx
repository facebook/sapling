/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PendingComment} from './pendingCommentsState';

import * as stylex from '@stylexjs/stylex';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useCallback} from 'react';
import {colors} from '../../../components/theme/tokens.stylex';
import {removePendingComment} from './pendingCommentsState';

export type PendingCommentDisplayProps = {
  comment: PendingComment;
  prNumber: string;
  /** Optional callback when edit mode is triggered (future enhancement) */
  onEdit?: () => void;
};

const styles = stylex.create({
  container: {
    display: 'flex',
    flexDirection: 'column',
    gap: '10px',
    padding: '14px 16px',
    backgroundColor: 'rgba(92, 124, 250, 0.06)',
    border: '1px solid rgba(92, 124, 250, 0.2)',
    borderRadius: '8px',
    position: 'relative',
    borderLeft: '3px solid #5c7cfa',
  },
  header: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: '8px',
  },
  headerLeft: {
    display: 'flex',
    alignItems: 'center',
    gap: '10px',
  },
  badge: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '5px',
    padding: '3px 8px',
    backgroundColor: 'rgba(92, 124, 250, 0.15)',
    color: '#5c7cfa',
    borderRadius: '4px',
    fontSize: '11px',
    fontWeight: '600',
    textTransform: 'uppercase',
    letterSpacing: '0.3px',
  },
  badgeIcon: {
    fontSize: '11px',
  },
  typeLabel: {
    fontSize: '11px',
    color: 'var(--graphite-text-tertiary)',
    textTransform: 'uppercase',
    letterSpacing: '0.5px',
  },
  location: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '6px',
    fontSize: '11px',
    color: 'var(--graphite-text-secondary)',
    fontFamily: 'var(--monospace-fontFamily)',
    backgroundColor: 'rgba(0, 0, 0, 0.2)',
    padding: '4px 8px',
    borderRadius: '4px',
    maxWidth: '100%',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  body: {
    whiteSpace: 'pre-wrap',
    wordBreak: 'break-word',
    color: colors.fg,
    fontSize: '13px',
    lineHeight: '1.5',
    padding: '8px 0',
  },
  actions: {
    display: 'flex',
    gap: '6px',
    justifyContent: 'flex-end',
    paddingTop: '4px',
    borderTop: '1px solid rgba(255, 255, 255, 0.05)',
    marginTop: '2px',
  },
  actionBtn: {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: '28px',
    height: '28px',
    borderRadius: '5px',
    backgroundColor: 'transparent',
    border: '1px solid rgba(255, 255, 255, 0.08)',
    color: 'var(--graphite-text-tertiary)',
    cursor: 'pointer',
    transition: 'all 0.15s ease',
    ':hover': {
      backgroundColor: 'rgba(255, 255, 255, 0.08)',
      borderColor: 'rgba(255, 255, 255, 0.12)',
      color: 'var(--graphite-text-secondary)',
    },
  },
  deleteBtn: {
    ':hover': {
      backgroundColor: 'rgba(239, 68, 68, 0.12)',
      borderColor: 'rgba(239, 68, 68, 0.3)',
      color: '#ef4444',
    },
  },
});

/**
 * Displays a pending comment with visual indication of "pending" state
 * and actions to delete (and in the future, edit) the comment.
 */
export function PendingCommentDisplay({
  comment,
  prNumber,
  onEdit,
}: PendingCommentDisplayProps) {
  const handleDelete = useCallback(() => {
    removePendingComment(prNumber, comment.id);
  }, [prNumber, comment.id]);

  const getTypeIcon = (): 'comment' | 'file' | 'comment-discussion' => {
    switch (comment.type) {
      case 'inline':
        return 'comment';
      case 'file':
        return 'file';
      case 'pr':
        return 'comment-discussion';
    }
  };

  const getTypeLabel = () => {
    switch (comment.type) {
      case 'inline':
        return 'Line';
      case 'file':
        return 'File';
      case 'pr':
        return 'Review';
    }
  };

  const getLocationText = () => {
    if (comment.type === 'inline' && comment.path && comment.line) {
      // Show just filename:line for brevity
      const filename = comment.path.split('/').pop() ?? comment.path;
      return `${filename}:${comment.line}`;
    }
    if (comment.type === 'file' && comment.path) {
      return comment.path.split('/').pop() ?? comment.path;
    }
    return null;
  };

  const locationText = getLocationText();

  return (
    <div {...stylex.props(styles.container)}>
      <div {...stylex.props(styles.header)}>
        <div {...stylex.props(styles.headerLeft)}>
          <div {...stylex.props(styles.badge)}>
            <Icon icon="clock" />
            <span>Pending</span>
          </div>
          <span {...stylex.props(styles.typeLabel)}>{getTypeLabel()}</span>
        </div>
      </div>

      {locationText != null && (
        <span {...stylex.props(styles.location)}>
          <Icon icon={getTypeIcon()} />
          {locationText}
        </span>
      )}

      <div {...stylex.props(styles.body)}>{comment.body}</div>

      <div {...stylex.props(styles.actions)}>
        {onEdit != null && (
          <Tooltip title="Edit comment">
            <button {...stylex.props(styles.actionBtn)} onClick={onEdit}>
              <Icon icon="edit" />
            </button>
          </Tooltip>
        )}
        <Tooltip title="Delete comment">
          <button {...stylex.props(styles.actionBtn, styles.deleteBtn)} onClick={handleDelete}>
            <Icon icon="trash" />
          </button>
        </Tooltip>
      </div>
    </div>
  );
}
