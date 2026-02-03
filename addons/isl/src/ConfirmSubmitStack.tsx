/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from './types';

import {Button} from 'isl-components/Button';
import {Checkbox} from 'isl-components/Checkbox';
import {Divider} from 'isl-components/Divider';
import {TextField} from 'isl-components/TextField';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom, useAtomValue} from 'jotai';
import {useState} from 'react';
import {useAutofocusRef} from 'shared/hooks';
import {nullthrows} from 'shared/utils';
import {Commit} from './Commit';
import {FlexSpacer} from './ComponentUtils';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {submitAsDraft, SubmitAsDraftCheckbox} from './codeReview/DraftCheckbox';
import {publishWhenReady, PublishWhenReadyCheckbox} from './codeReview/PublishWhenReadyCheckbox';
import {t, T} from './i18n';
import {configBackedAtom, readAtom} from './jotaiUtils';
import {CommitPreview} from './previews';
import {useModal} from './useModal';

import './ConfirmSubmitStack.css';

export const confirmShouldSubmitEnabledAtom = configBackedAtom<boolean>(
  'isl.show-stack-submit-confirmation',
  true,
);

export type SubmitConfirmationReponse =
  | {submitAsDraft: boolean; updateMessage?: string; publishWhenReady?: boolean}
  | undefined;

type SubmitType = 'submit' | 'submit-all' | 'resubmit';

export function shouldShowSubmitStackConfirmation(): boolean {
  const provider = readAtom(codeReviewProvider);
  const shouldShowConfirmation = readAtom(confirmShouldSubmitEnabledAtom);
  return (
    shouldShowConfirmation === true &&
    // if you can't submit as draft, no need to show the interstitial
    provider?.supportSubmittingAsDraft != null
  );
}

/**
 * Show a modal to confirm if you want to bulk submit a given stack of commits.
 * Allows you to set if you want to submit as a draft or not,
 * and provide an update message.
 *
 * If your code review provider does not support submitting as draft,
 * this function returns true immediately.
 */
export function useShowConfirmSubmitStack() {
  const showModal = useModal();

  return async (mode: SubmitType, stack: Array<CommitInfo>) => {
    if (!shouldShowSubmitStackConfirmation()) {
      const draft = readAtom(submitAsDraft);
      return {submitAsDraft: draft ?? false};
    }

    const provider = readAtom(codeReviewProvider);

    const replace = {
      $numCommits: String(stack.length),
      $cmd: nullthrows(provider).submitCommandName(),
    };
    const title =
      mode === 'submit'
        ? t('Submitting $numCommits commits for review with $cmd', {replace})
        : mode === 'resubmit'
          ? t('Submitting new versions of $numCommits commits for review with $cmd', {replace})
          : t('Submitting all $numCommits commits in this stack for review with $cmd', {replace});
    const response = await showModal<SubmitConfirmationReponse>({
      type: 'custom',
      title,
      component: ({returnResultAndDismiss}) => (
        <ConfirmModalContent stack={stack} returnResultAndDismiss={returnResultAndDismiss} />
      ),
    });
    return response;
  };
}

function ConfirmModalContent({
  stack,
  returnResultAndDismiss,
}: {
  stack: Array<CommitInfo>;
  returnResultAndDismiss: (value: SubmitConfirmationReponse) => unknown;
}) {
  const [showSubmitConfirmation, setShowSubmitConfirmation] = useAtom(
    confirmShouldSubmitEnabledAtom,
  );
  const shouldSubmitAsDraft = useAtomValue(submitAsDraft);
  const shouldPublishWhenReady = useAtomValue(publishWhenReady);
  const [updateMessage, setUpdateMessage] = useState('');
  const commitsWithDiffs = stack.filter(commit => commit.diffId != null);

  const submitRef = useAutofocusRef<HTMLButtonElement>();

  const provider = useAtomValue(codeReviewProvider);
  return (
    <div className="confirm-submit-stack" data-testid="confirm-submit-stack">
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
        {provider?.supportsUpdateMessage !== true || commitsWithDiffs.length === 0 ? null : (
          <TextField
            value={updateMessage}
            data-testid="submit-update-message-input"
            onChange={e => setUpdateMessage(e.currentTarget.value)}>
            Update Message
          </TextField>
        )}
        <SubmitAsDraftCheckbox commitsToBeSubmit={stack} />
        <PublishWhenReadyCheckbox />
      </div>
      <Divider />
      <div className="use-modal-buttons">
        <Tooltip
          placement="bottom"
          title={t(
            "Don't show this confirmation next time you submit a stack. " +
              'Your last setting will control if it is submitted as a draft. ' +
              'You can change this from settings.',
          )}>
          <Checkbox
            checked={!showSubmitConfirmation}
            onChange={checked => setShowSubmitConfirmation(!checked)}>
            <T>Don't show again</T>
          </Checkbox>
        </Tooltip>
        <FlexSpacer />
        <Button onClick={() => returnResultAndDismiss(undefined)}>
          <T>Cancel</T>
        </Button>
        <Button
          ref={submitRef}
          primary
          onClick={() =>
            returnResultAndDismiss({
              submitAsDraft: shouldSubmitAsDraft,
              updateMessage: updateMessage || undefined,
              publishWhenReady: shouldPublishWhenReady,
            })
          }>
          <T>Submit</T>
        </Button>
      </div>
    </div>
  );
}
