/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PendingComment} from './pendingCommentsState';

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {useState, useCallback} from 'react';
import {colors, spacing, radius, font} from '../../../components/theme/tokens.stylex';
import {addPendingComment} from './pendingCommentsState';

export type CommentInputProps = {
  prNumber: string;
  type: 'inline' | 'file' | 'pr';
  /** File path - required for inline/file comments */
  path?: string;
  /** Line number - required for inline comments */
  line?: number;
  /** Which side of the diff (LEFT = old, RIGHT = new) */
  side?: 'LEFT' | 'RIGHT';
  /** Called when the comment input is cancelled or submitted */
  onCancel: () => void;
  /** Optional callback after successfully adding a comment */
  onSubmit?: () => void;
};

const styles = stylex.create({
  container: {
    display: 'flex',
    flexDirection: 'column',
    gap: spacing.half,
    padding: spacing.pad,
    backgroundColor: colors.bg,
    border: '1px solid var(--graphite-border)',
    borderRadius: radius.round,
  },
  textarea: {
    minHeight: '60px',
    resize: 'vertical',
    padding: spacing.half,
    backgroundColor: 'var(--graphite-bg-subtle)',
    color: colors.fg,
    border: '1px solid var(--graphite-border-subtle)',
    borderRadius: radius.small,
    fontFamily: 'inherit',
    fontSize: font.normal,
    ':focus': {
      outline: 'none',
      borderColor: 'var(--graphite-accent)',
    },
  },
  buttons: {
    display: 'flex',
    gap: spacing.half,
    justifyContent: 'flex-end',
  },
  label: {
    fontSize: font.small,
    color: 'var(--graphite-text-secondary)',
  },
});

/**
 * Input component for authoring new pending review comments.
 * Supports inline (line-level), file-level, and PR-level comments.
 */
export function CommentInput({
  prNumber,
  type,
  path,
  line,
  side,
  onCancel,
  onSubmit,
}: CommentInputProps) {
  const [body, setBody] = useState('');

  const handleSubmit = useCallback(() => {
    if (body.trim() === '') {
      return;
    }

    const comment: Omit<PendingComment, 'id' | 'createdAt'> = {
      type,
      body: body.trim(),
      ...(path != null && {path}),
      ...(line != null && {line}),
      ...(side != null && {side}),
    };

    addPendingComment(prNumber, comment);
    onSubmit?.();
    onCancel();
  }, [body, prNumber, type, path, line, side, onSubmit, onCancel]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      // Cmd/Ctrl + Enter to submit
      if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
        e.preventDefault();
        handleSubmit();
      }
      // Escape to cancel
      if (e.key === 'Escape') {
        e.preventDefault();
        onCancel();
      }
    },
    [handleSubmit, onCancel],
  );

  const getLabel = () => {
    switch (type) {
      case 'inline':
        return `Comment on line ${line}`;
      case 'file':
        return 'File comment';
      case 'pr':
        return 'Review comment';
    }
  };

  return (
    <div {...stylex.props(styles.container)}>
      <span {...stylex.props(styles.label)}>{getLabel()}</span>
      <textarea
        {...stylex.props(styles.textarea)}
        value={body}
        onChange={e => setBody(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder="Leave a comment..."
        autoFocus
      />
      <div {...stylex.props(styles.buttons)}>
        <Button onClick={onCancel}>Cancel</Button>
        <Button primary disabled={body.trim() === ''} onClick={handleSubmit}>
          Add comment
        </Button>
      </div>
    </div>
  );
}
