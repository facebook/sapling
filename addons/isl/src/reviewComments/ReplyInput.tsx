/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {useState, useCallback} from 'react';
import {colors, spacing, radius, font} from '../../../components/theme/tokens.stylex';
import serverAPI from '../ClientToServerAPI';
import {T} from '../i18n';

export type ReplyInputProps = {
  /** GitHub thread node ID for the reply */
  threadId: string;
  /** Called when the reply input is cancelled */
  onCancel: () => void;
  /** Called after successfully submitting a reply */
  onSuccess?: () => void;
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
    marginTop: spacing.half,
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
    alignItems: 'center',
  },
  error: {
    color: 'var(--graphite-error)',
    fontSize: font.small,
    marginRight: 'auto',
  },
  label: {
    fontSize: font.small,
    color: 'var(--graphite-text-secondary)',
  },
});

/**
 * Input component for replying to existing comment threads.
 * Replies are submitted immediately via GraphQL mutation (not batched).
 */
export function ReplyInput({threadId, onCancel, onSuccess}: ReplyInputProps) {
  const [body, setBody] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = useCallback(async () => {
    if (body.trim() === '' || isSubmitting) {
      return;
    }

    setIsSubmitting(true);
    setError(null);

    try {
      serverAPI.postMessage({
        type: 'graphqlReply',
        threadId,
        body: body.trim(),
      });

      const result = await serverAPI.nextMessageMatching(
        'graphqlReplyResult',
        msg => msg.threadId === threadId,
      );

      if (result.error) {
        throw new Error(result.error);
      }

      onSuccess?.();
      onCancel();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to submit reply');
    } finally {
      setIsSubmitting(false);
    }
  }, [body, isSubmitting, threadId, onSuccess, onCancel]);

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

  return (
    <div {...stylex.props(styles.container)}>
      <span {...stylex.props(styles.label)}>
        <T>Reply to thread</T>
      </span>
      <textarea
        {...stylex.props(styles.textarea)}
        value={body}
        onChange={e => setBody(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder="Write a reply..."
        autoFocus
        disabled={isSubmitting}
      />
      <div {...stylex.props(styles.buttons)}>
        {error && <span {...stylex.props(styles.error)}>{error}</span>}
        <Button onClick={onCancel} disabled={isSubmitting}>
          <T>Cancel</T>
        </Button>
        <Button
          primary
          disabled={body.trim() === '' || isSubmitting}
          onClick={handleSubmit}>
          {isSubmitting ? <Icon icon="loading" /> : <T>Reply</T>}
        </Button>
      </div>
    </div>
  );
}
