/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PendingComment} from './pendingCommentsState';

import * as stylex from '@stylexjs/stylex';
import {Icon} from 'isl-components/Icon';
import {useState, useCallback, useRef, useEffect} from 'react';
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

const fadeIn = stylex.keyframes({
  '0%': {opacity: 0, transform: 'translateY(-4px)'},
  '100%': {opacity: 1, transform: 'translateY(0)'},
});

const styles = stylex.create({
  container: {
    display: 'flex',
    flexDirection: 'column',
    gap: '12px',
    padding: '16px',
    backgroundColor: 'rgba(30, 34, 42, 0.95)',
    border: '1px solid rgba(92, 124, 250, 0.3)',
    borderRadius: '8px',
    boxShadow: '0 4px 20px rgba(0, 0, 0, 0.3), 0 0 0 1px rgba(255, 255, 255, 0.03)',
    animationName: fadeIn,
    animationDuration: '0.2s',
    animationTimingFunction: 'ease-out',
  },
  header: {
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
  },
  typeIcon: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: '28px',
    height: '28px',
    borderRadius: '6px',
    backgroundColor: 'rgba(92, 124, 250, 0.15)',
    color: '#5c7cfa',
  },
  labelContainer: {
    display: 'flex',
    flexDirection: 'column',
    gap: '2px',
  },
  label: {
    fontSize: '13px',
    fontWeight: '500',
    color: colors.fg,
    letterSpacing: '-0.01em',
  },
  sublabel: {
    fontSize: '11px',
    color: 'var(--graphite-text-tertiary)',
    fontFamily: 'var(--monospace-fontFamily)',
  },
  textareaWrapper: {
    position: 'relative',
  },
  textarea: {
    width: '100%',
    minHeight: '80px',
    resize: 'vertical',
    padding: '12px 14px',
    backgroundColor: 'rgba(0, 0, 0, 0.25)',
    color: colors.fg,
    border: '1px solid rgba(255, 255, 255, 0.08)',
    borderRadius: '6px',
    fontFamily: 'inherit',
    fontSize: '13px',
    lineHeight: '1.5',
    transition: 'border-color 0.15s ease, box-shadow 0.15s ease',
    '::placeholder': {
      color: 'var(--graphite-text-tertiary)',
    },
    ':focus': {
      outline: 'none',
      borderColor: 'rgba(92, 124, 250, 0.5)',
      boxShadow: '0 0 0 3px rgba(92, 124, 250, 0.1)',
    },
  },
  footer: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  hint: {
    fontSize: '11px',
    color: 'var(--graphite-text-tertiary)',
    display: 'flex',
    alignItems: 'center',
    gap: '4px',
  },
  kbd: {
    display: 'inline-flex',
    alignItems: 'center',
    padding: '2px 5px',
    fontSize: '10px',
    fontFamily: 'inherit',
    backgroundColor: 'rgba(255, 255, 255, 0.06)',
    borderRadius: '3px',
    border: '1px solid rgba(255, 255, 255, 0.08)',
    color: 'var(--graphite-text-secondary)',
  },
  buttons: {
    display: 'flex',
    gap: '8px',
  },
  cancelBtn: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '6px',
    padding: '7px 14px',
    fontSize: '12px',
    fontWeight: '500',
    color: 'var(--graphite-text-secondary)',
    backgroundColor: 'transparent',
    border: '1px solid rgba(255, 255, 255, 0.1)',
    borderRadius: '6px',
    cursor: 'pointer',
    transition: 'all 0.15s ease',
    ':hover': {
      backgroundColor: 'rgba(255, 255, 255, 0.05)',
      borderColor: 'rgba(255, 255, 255, 0.15)',
      color: colors.fg,
    },
  },
  submitBtn: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '6px',
    padding: '7px 16px',
    fontSize: '12px',
    fontWeight: '500',
    color: '#5c7cfa',
    backgroundColor: 'transparent',
    border: '1.5px solid #5c7cfa',
    borderRadius: '6px',
    cursor: 'pointer',
    transition: 'all 0.15s ease',
    ':hover': {
      backgroundColor: 'rgba(92, 124, 250, 0.15)',
      borderColor: '#748ffc',
      color: '#748ffc',
    },
  },
  submitBtnDisabled: {
    opacity: 0.4,
    cursor: 'not-allowed',
    ':hover': {
      backgroundColor: 'transparent',
      borderColor: '#5c7cfa',
      color: '#5c7cfa',
    },
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
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Auto-focus the textarea on mount
  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

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

  const getIcon = (): 'comment' | 'file' | 'comment-discussion' => {
    switch (type) {
      case 'inline':
        return 'comment';
      case 'file':
        return 'file';
      case 'pr':
        return 'comment-discussion';
    }
  };

  const getLabel = () => {
    switch (type) {
      case 'inline':
        return 'Line comment';
      case 'file':
        return 'File comment';
      case 'pr':
        return 'Review comment';
    }
  };

  const getSublabel = () => {
    switch (type) {
      case 'inline':
        return `Line ${line}${side === 'LEFT' ? ' (old)' : side === 'RIGHT' ? ' (new)' : ''}`;
      case 'file':
        return path ? path.split('/').pop() : undefined;
      case 'pr':
        return undefined;
    }
  };

  const sublabel = getSublabel();
  const canSubmit = body.trim() !== '';

  return (
    <div {...stylex.props(styles.container)}>
      <div {...stylex.props(styles.header)}>
        <div {...stylex.props(styles.typeIcon)}>
          <Icon icon={getIcon()} />
        </div>
        <div {...stylex.props(styles.labelContainer)}>
          <span {...stylex.props(styles.label)}>{getLabel()}</span>
          {sublabel && <span {...stylex.props(styles.sublabel)}>{sublabel}</span>}
        </div>
      </div>

      <div {...stylex.props(styles.textareaWrapper)}>
        <textarea
          ref={textareaRef}
          {...stylex.props(styles.textarea)}
          value={body}
          onChange={e => setBody(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Write your comment..."
        />
      </div>

      <div {...stylex.props(styles.footer)}>
        <div {...stylex.props(styles.hint)}>
          <span {...stylex.props(styles.kbd)}>⌘</span>
          <span {...stylex.props(styles.kbd)}>↵</span>
          <span>to submit</span>
        </div>
        <div {...stylex.props(styles.buttons)}>
          <button {...stylex.props(styles.cancelBtn)} onClick={onCancel}>
            Cancel
          </button>
          <button
            {...stylex.props(styles.submitBtn, !canSubmit && styles.submitBtnDisabled)}
            onClick={handleSubmit}
            disabled={!canSubmit}>
            <Icon icon="check" />
            Add comment
          </button>
        </div>
      </div>
    </div>
  );
}
