/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PullRequestReviewEvent} from '../types';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {TextArea} from 'isl-components/TextArea';
import {useState} from 'react';
import {T, t} from '../i18n';

import './ReviewSubmissionModal.css';

export type ReviewSubmissionResult = {
  event: PullRequestReviewEvent;
  body: string;
};

type Props = {
  pendingCommentCount: number;
  returnResultAndDismiss: (result: ReviewSubmissionResult | undefined) => void;
};

/**
 * Modal for submitting a PR review with summary text and approval action.
 * Displayed via useModal with type: 'custom'.
 */
export function ReviewSubmissionModal({pendingCommentCount, returnResultAndDismiss}: Props) {
  const [body, setBody] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);

  const handleSubmit = (event: PullRequestReviewEvent) => {
    setIsSubmitting(true);
    returnResultAndDismiss({event, body});
  };

  const handleCancel = () => {
    returnResultAndDismiss(undefined);
  };

  return (
    <div className="review-submission-modal">
      <div className="review-submission-header">
        <Icon icon="comment-discussion" size="M" />
        <span><T>Submit Review</T></span>
      </div>

      <div className="review-submission-body">
        <label htmlFor="review-summary" className="review-summary-label">
          <T>Review Summary</T>
          <span className="optional-hint">(<T>optional</T>)</span>
        </label>
        <TextArea
          id="review-summary"
          value={body}
          onChange={e => setBody(e.currentTarget.value)}
          placeholder={t('Add a summary comment for your review...')}
          rows={4}
          disabled={isSubmitting}
          data-testid="review-summary-input"
        />

        {pendingCommentCount > 0 && (
          <div className="pending-comments-info">
            <Icon icon="comment" />
            <span>
              <T replace={{$count: String(pendingCommentCount)}}>
                $count pending comment(s) will be submitted
              </T>
            </span>
          </div>
        )}
      </div>

      <div className="review-submission-actions">
        <Button onClick={handleCancel} disabled={isSubmitting}>
          <T>Cancel</T>
        </Button>
        <Button
          onClick={() => handleSubmit('COMMENT')}
          disabled={isSubmitting}
          data-testid="submit-comment-button">
          <Icon icon="comment" slot="start" />
          <T>Comment</T>
        </Button>
        <Button
          onClick={() => handleSubmit('REQUEST_CHANGES')}
          disabled={isSubmitting}
          data-testid="submit-request-changes-button">
          <Icon icon="diff" slot="start" />
          <T>Request Changes</T>
        </Button>
        <Button
          onClick={() => handleSubmit('APPROVE')}
          disabled={isSubmitting}
          kind="primary"
          data-testid="submit-approve-button">
          <Icon icon="check" slot="start" />
          <T>Approve</T>
        </Button>
      </div>
    </div>
  );
}
