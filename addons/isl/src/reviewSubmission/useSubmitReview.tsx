/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DraftPullRequestReviewThread, PullRequestReviewEvent} from '../types';
import type {ReviewSubmissionResult} from './ReviewSubmissionModal';

import {useAtomValue} from 'jotai';
import {useCallback} from 'react';
import serverAPI from '../ClientToServerAPI';
import {triggerFullDiffSummariesRefresh} from '../codeReview/CodeReviewInfo';
import {t} from '../i18n';
import {writeAtom} from '../jotaiUtils';
import {
  pendingCommentsAtom,
  clearPendingComments,
} from '../reviewComments/pendingCommentsState';
import {exitReviewMode, reviewModeAtom} from '../reviewMode';
import {showToast} from '../toast';
import {useModal} from '../useModal';
import {ReviewSubmissionModal} from './ReviewSubmissionModal';
import {reviewSubmittedAtom} from './reviewSubmittedState';

/**
 * Hook that provides a function to initiate the review submission flow.
 * Shows modal, submits to GitHub, clears state, and exits review mode.
 *
 * @param nodeId - GitHub GraphQL node ID for the PR (from GitHubDiffSummary.nodeId)
 */
export function useSubmitReview(nodeId: string | undefined) {
  const showModal = useModal();
  const reviewMode = useAtomValue(reviewModeAtom);
  const prNumber = reviewMode.prNumber;
  const pendingComments = useAtomValue(pendingCommentsAtom(prNumber ?? ''));

  const submitReview = useCallback(async () => {
    if (!nodeId || !prNumber) {
      showToast(t('Cannot submit review: missing PR information'), {durationMs: 3000});
      return;
    }

    // Convert pending comments to GraphQL thread format
    // GitHub convention: 'line' is the end line, 'startLine' is the first line of a range
    const threads: DraftPullRequestReviewThread[] = pendingComments
      .filter(c => c.type === 'inline' && c.path && c.line)
      .map(c => ({
        path: c.path!,
        line: c.endLine ?? c.line!,
        ...(c.endLine != null && c.endLine !== c.line && {startLine: c.line!}),
        body: c.body,
        side: c.side,
      }));

    // Show the submission modal
    const result = await showModal<ReviewSubmissionResult | undefined>({
      type: 'custom',
      title: t('Submit Review'),
      width: 500,
      component: ({returnResultAndDismiss}) => (
        <ReviewSubmissionModal
          pendingCommentCount={pendingComments.length}
          returnResultAndDismiss={returnResultAndDismiss}
        />
      ),
    });

    // User cancelled
    if (!result) {
      return;
    }

    // Submit to GitHub
    serverAPI.postMessage({
      type: 'submitPullRequestReview',
      pullRequestId: nodeId,
      event: result.event,
      body: result.body || undefined,
      threads: threads.length > 0 ? threads : undefined,
    });

    // Wait for response
    const response = await serverAPI.nextMessageMatching(
      'submittedPullRequestReview',
      () => true,
    );

    if (response.result.error) {
      showToast(t('Failed to submit review: $error', {replace: {$error: response.result.error.message}}), {
        durationMs: 5000,
      });
      return;
    }

    // Success! Clear pending comments and exit review mode
    clearPendingComments(prNumber);
    writeAtom(reviewSubmittedAtom, {event: result.event, timestamp: new Date()});
    triggerFullDiffSummariesRefresh();

    const actionText = toastActionText(result.event);
    const toastClass = result.event === 'APPROVE' ? 'toast-review-approved' : result.event === 'REQUEST_CHANGES' ? 'toast-review-changes' : undefined;
    showToast(t('Review submitted: $action PR #$pr', {
      replace: {$action: actionText, $pr: prNumber},
    }), {durationMs: 5000, className: toastClass});

    exitReviewMode();
  }, [nodeId, prNumber, pendingComments, showModal]);

  return {
    submitReview,
    canSubmit: !!nodeId && !!prNumber,
    pendingCommentCount: pendingComments.length,
  };
}

/**
 * Hook for quick review actions (Approve / Request Changes) without modal.
 * Used for the quick action buttons in the header.
 */
export function useQuickReviewAction(nodeId: string | undefined) {
  const reviewMode = useAtomValue(reviewModeAtom);
  const prNumber = reviewMode.prNumber;
  const pendingComments = useAtomValue(pendingCommentsAtom(prNumber ?? ''));

  const submitQuickReview = useCallback(async (event: 'APPROVE' | 'REQUEST_CHANGES') => {
    if (!nodeId || !prNumber) {
      showToast(t('Cannot submit review: missing PR information'), {durationMs: 3000});
      return;
    }

    // Convert pending comments to GraphQL thread format
    // GitHub convention: 'line' is the end line, 'startLine' is the first line of a range
    const threads = pendingComments
      .filter(c => c.type === 'inline' && c.path && c.line)
      .map(c => ({
        path: c.path!,
        line: c.endLine ?? c.line!,
        ...(c.endLine != null && c.endLine !== c.line && {startLine: c.line!}),
        body: c.body,
        side: c.side,
      }));

    // Submit directly to GitHub
    serverAPI.postMessage({
      type: 'submitPullRequestReview',
      pullRequestId: nodeId,
      event,
      body: undefined,
      threads: threads.length > 0 ? threads : undefined,
    });

    // Wait for response
    const response = await serverAPI.nextMessageMatching(
      'submittedPullRequestReview',
      () => true,
    );

    if (response.result.error) {
      showToast(t('Failed to submit review: $error', {replace: {$error: response.result.error.message}}), {
        durationMs: 5000,
      });
      return;
    }

    // Success! Update local state, clear comments, refresh data
    clearPendingComments(prNumber);
    writeAtom(reviewSubmittedAtom, {event, timestamp: new Date()});
    triggerFullDiffSummariesRefresh();

    const actionText = toastActionText(event);
    const toastClass = event === 'APPROVE' ? 'toast-review-approved' : event === 'REQUEST_CHANGES' ? 'toast-review-changes' : undefined;
    showToast(t('Review submitted: $action PR #$pr', {
      replace: {$action: actionText, $pr: prNumber},
    }), {durationMs: 5000, className: toastClass});
  }, [nodeId, prNumber, pendingComments]);

  return {
    approve: useCallback(() => submitQuickReview('APPROVE'), [submitQuickReview]),
    requestChanges: useCallback(() => submitQuickReview('REQUEST_CHANGES'), [submitQuickReview]),
    canSubmit: !!nodeId && !!prNumber,
  };
}

function toastActionText(event: PullRequestReviewEvent): string {
  switch (event) {
    case 'APPROVE':
      return t('approved');
    case 'REQUEST_CHANGES':
      return t('requested changes on');
    default:
      return t('commented on');
  }
}
