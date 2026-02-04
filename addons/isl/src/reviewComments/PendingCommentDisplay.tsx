/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PendingComment} from './pendingCommentsState';

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useCallback} from 'react';
import {colors, spacing, radius, font} from '../../../components/theme/tokens.stylex';
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
    gap: spacing.half,
    padding: spacing.pad,
    backgroundColor: 'var(--graphite-bg-subtle)',
    border: '1px dashed var(--graphite-border)',
    borderRadius: radius.round,
    position: 'relative',
  },
  header: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: spacing.half,
  },
  badge: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: spacing.quarter,
    padding: `${spacing.quarter} ${spacing.half}`,
    backgroundColor: 'var(--graphite-accent-subtle)',
    color: 'var(--graphite-accent)',
    borderRadius: radius.small,
    fontSize: font.small,
    fontWeight: 500,
  },
  typeLabel: {
    fontSize: font.smaller,
    color: 'var(--graphite-text-tertiary)',
    textTransform: 'uppercase',
    letterSpacing: '0.5px',
  },
  body: {
    whiteSpace: 'pre-wrap',
    wordBreak: 'break-word',
    color: colors.fg,
    fontSize: font.normal,
    lineHeight: 1.4,
  },
  actions: {
    display: 'flex',
    gap: spacing.half,
    justifyContent: 'flex-end',
  },
  deleteButton: {
    opacity: 0.6,
    transition: 'opacity 0.15s ease',
    ':hover': {
      opacity: 1,
    },
  },
  location: {
    fontSize: font.smaller,
    color: 'var(--graphite-text-muted)',
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
      return `${comment.path}:${comment.line}`;
    }
    if (comment.type === 'file' && comment.path) {
      return comment.path;
    }
    return null;
  };

  const locationText = getLocationText();

  return (
    <div {...stylex.props(styles.container)}>
      <div {...stylex.props(styles.header)}>
        <div {...stylex.props(styles.badge)}>
          <Icon icon={getTypeIcon()} />
          <span>Pending</span>
        </div>
        <span {...stylex.props(styles.typeLabel)}>{getTypeLabel()}</span>
      </div>

      {locationText != null && (
        <span {...stylex.props(styles.location)}>{locationText}</span>
      )}

      <div {...stylex.props(styles.body)}>{comment.body}</div>

      <div {...stylex.props(styles.actions)}>
        {onEdit != null && (
          <Tooltip title="Edit comment">
            <Button xstyle={styles.deleteButton} onClick={onEdit}>
              <Icon icon="edit" />
            </Button>
          </Tooltip>
        )}
        <Tooltip title="Delete comment">
          <Button xstyle={styles.deleteButton} onClick={handleDelete}>
            <Icon icon="trash" />
          </Button>
        </Tooltip>
      </div>
    </div>
  );
}
