/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UICodeReviewProvider} from './codeReview/UICodeReviewProvider';
import type {CommitInfo} from './types';
import type {useModal} from './useModal';

import {Commit} from './Commit';
import {FlexSpacer} from './ComponentUtils';
import {Tooltip} from './Tooltip';
import {VSCodeCheckbox} from './VSCodeCheckbox';
import {SubmitAsDraftCheckbox} from './codeReview/DraftCheckbox';
import {t, T} from './i18n';
import {CommitPreview} from './previews';
import {VSCodeDivider, VSCodeButton} from '@vscode/webview-ui-toolkit/react';

import './ConfirmSubmitStack.css';

/**
 * Show a modal to confirm if you want to bulk submit a given stack of commits.
 * Allows you to set if you want to submit as a draft or not,
 * and provide an update message (TODO).
 *
 * If your code review provider does not support submitting as draft,
 * this function returns true immediately.
 */
export async function confirmShouldSubmit(
  mode: 'submit' | 'submit-all' | 'resubmit',
  showModal: ReturnType<typeof useModal>,
  provider: UICodeReviewProvider,
  stack: Array<CommitInfo>,
): Promise<boolean> {
  if (!provider.supportSubmittingAsDraft) {
    // if you can't submit as draft, no need to show the interstitial
    return true;
  }
  function ConfirmModalContent({
    returnResultAndDismiss,
  }: {
    returnResultAndDismiss: (value: boolean) => unknown;
  }) {
    return (
      <div className="confirm-submit-stack">
        <div className="confirm-submit-stack-content">
          <div className="commit-list">
            {stack.map(commit => (
              <Commit
                key={commit.hash}
                commit={commit}
                hasChildren={false}
                previewType={CommitPreview.NON_ACTIONABLE_COMMIT}
              />
            ))}
          </div>
          <SubmitAsDraftCheckbox commitsToBeSubmit={stack} />
        </div>
        <VSCodeDivider />
        <div className="use-modal-buttons">
          <Tooltip
            placement="bottom"
            title={t(
              "Don't show this confirmation next time you submit a stack. " +
                'Your last setting will control if it is submitted as a draft. ' +
                'You can change this from settings.',
            )}>
            <VSCodeCheckbox checked={/* TODO: set as a setting */ false}>
              <T>Don't show again</T>
            </VSCodeCheckbox>
          </Tooltip>
          <FlexSpacer />
          <VSCodeButton appearance="secondary" onClick={() => returnResultAndDismiss(false)}>
            <T>Cancel</T>
          </VSCodeButton>
          <VSCodeButton appearance="primary" autofocus onClick={() => returnResultAndDismiss(true)}>
            <T>Submit</T>
          </VSCodeButton>
        </div>
      </div>
    );
  }
  const replace = {$numCommits: String(stack.length)};
  const title =
    mode === 'submit'
      ? t('Submit $numCommits commits for review?', {replace})
      : mode === 'resubmit'
      ? t('Resubmit $numCommits commits that already have diffs for review?', {replace})
      : t('Submit all $numCommits commits in this stack for review?', {replace});
  const response = await showModal<boolean>({
    type: 'custom',
    title,
    component: ConfirmModalContent,
  });
  return response === true;
}
