/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangeEvent, KeyboardEvent} from 'react';

import {Box, Button, Flash, Textarea} from '@primer/react';
import {useCallback, useRef, useState} from 'react';

type Props = {
  /**
   * Returning a rejected Promise indicates the user should be allowed to try
   * to submit the form again.
   */
  addComment: (comment: string) => Promise<void>;
  /**
   * true if the component should still be rendered after the comment is added
   * successfully; false if the component is expected to be unmounted after the
   * comment is added successfully.
   */
  resetInputAfterAddingComment: boolean;
  autoFocus: boolean;
  onCancel?: () => void;
  allowEmptyMessage?: boolean;
  label?: string;
  actionSelector?: React.ReactNode;
};

/**
 * Convert API error messages to user-friendly messages.
 */
function formatErrorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);

  // Handle specific GitHub API errors with user-friendly messages
  if (message.includes('end commit oid is not part of the pull request')) {
    return 'Cannot add comment: the commit you are viewing is no longer part of this pull request. This can happen after a force push. Try refreshing and viewing the latest version.';
  }

  if (message.includes('client not found')) {
    return 'Cannot add comment: not connected to GitHub. Please try refreshing the page.';
  }

  if (message.includes('pull request not found') || message.includes('pull request id not found')) {
    return 'Cannot add comment: pull request not found. Please try refreshing the page.';
  }

  return `Failed to add comment: ${message}`;
}

export default function PullRequestCommentInput({
  addComment,
  resetInputAfterAddingComment,
  autoFocus,
  onCancel,
  allowEmptyMessage = false,
  label = 'Add Comment',
  actionSelector,
}: Props): React.ReactElement {
  const [comment, setComment] = useState<string>('');
  const [disabled, setDisabled] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const onChange = useCallback(
    (e: ChangeEvent<HTMLTextAreaElement>) => {
      const value = e.currentTarget.value;
      setComment(value);
      // Clear error when user starts typing again
      if (error != null) {
        setError(null);
      }
    },
    [setComment, error],
  );

  const onAddComment = useCallback(async () => {
    setDisabled(true);
    setError(null);

    try {
      await addComment(comment);
    } catch (e) {
      const errorMessage = formatErrorMessage(e);
      setError(errorMessage);
      // If adding the comment fails, let the user try again.
      setDisabled(false);
      return;
    }

    if (resetInputAfterAddingComment) {
      setComment('');
      setDisabled(false);
    }
  }, [addComment, resetInputAfterAddingComment, comment, setDisabled, setComment]);

  const isAddCommentDisabled = disabled || (!allowEmptyMessage && comment.trim() === '');

  // Use a ref to avoid stale closure in onKeyDown
  const isAddCommentDisabledRef = useRef(isAddCommentDisabled);
  isAddCommentDisabledRef.current = isAddCommentDisabled;

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      // Command+Enter (Mac) or Ctrl+Enter (Windows/Linux) to submit
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        if (!isAddCommentDisabledRef.current) {
          onAddComment();
        }
      }
    },
    [onAddComment],
  );

  const cancelButton =
    onCancel != null ? (
      <Button variant="danger" onClick={onCancel} disabled={disabled}>
        Cancel
      </Button>
    ) : null;

  return (
    <Box
      borderColor="border.default"
      borderTopWidth={1}
      borderTopStyle="solid"
      padding={1}
      width="100%">
      {error != null && (
        <Flash variant="danger" sx={{marginBottom: 2}}>
          {error}
        </Flash>
      )}
      <Textarea
        value={comment}
        onChange={onChange}
        onKeyDown={onKeyDown}
        placeholder="Write a comment..."
        block={true}
        autoFocus={autoFocus}
        resize="none"
        sx={{height: '80px', marginBottom: 1}}
      />
      <Box display="flex" justifyContent="flex-end" gridGap={1}>
        {actionSelector}
        {cancelButton}
        <Button disabled={isAddCommentDisabled} onClick={onAddComment} variant="primary">
          {label}
        </Button>
      </Box>
    </Box>
  );
}
