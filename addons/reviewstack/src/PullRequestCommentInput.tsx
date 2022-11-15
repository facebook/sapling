/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangeEvent} from 'react';

import {Box, Button, Textarea} from '@primer/react';
import {useCallback, useState} from 'react';

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

  const onChange = useCallback(
    (e: ChangeEvent<HTMLTextAreaElement>) => {
      const value = e.currentTarget.value;
      setComment(value);
    },
    [setComment],
  );

  const onAddComment = useCallback(async () => {
    setDisabled(true);

    try {
      await addComment(comment);
    } catch (e) {
      // TODO: Show dialog box to user rather than just dump it to the console
      // for debugging?
      // eslint-disable-next-line no-console
      console.error('addComment failed', e);
      // If adding the comment fails, let the user try again.
      setDisabled(false);
      return;
    }

    if (resetInputAfterAddingComment) {
      setComment('');
      setDisabled(false);
    }
  }, [addComment, resetInputAfterAddingComment, comment, setDisabled, setComment]);

  const cancelButton =
    onCancel != null ? (
      <Button variant="danger" onClick={onCancel} disabled={disabled}>
        Cancel
      </Button>
    ) : null;

  const isAddCommentDisabled = disabled || (!allowEmptyMessage && comment.trim() === '');
  return (
    <Box
      borderColor="border.default"
      borderTopWidth={1}
      borderTopStyle="solid"
      padding={1}
      width="100%">
      <Textarea
        value={comment}
        onChange={onChange}
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
