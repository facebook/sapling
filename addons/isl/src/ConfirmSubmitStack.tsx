/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from './types';
import type {MutableRefObject} from 'react';
import type {Snapshot} from 'recoil';

import {Commit} from './Commit';
import {SeeMoreContainer} from './CommitInfoView/SeeMoreContainer';
import {FlexSpacer} from './ComponentUtils';
import {Tooltip} from './Tooltip';
import {VSCodeCheckbox} from './VSCodeCheckbox';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {submitAsDraft, SubmitAsDraftCheckbox} from './codeReview/DraftCheckbox';
import {t, T} from './i18n';
import {persistAtomToConfigEffect} from './persistAtomToConfigEffect';
import {CommitPreview} from './previews';
import {useModal} from './useModal';
import {VSCodeDivider, VSCodeButton, VSCodeTextField} from '@vscode/webview-ui-toolkit/react';
import {useState} from 'react';
import {atom, useRecoilCallback, useRecoilState, useRecoilValue} from 'recoil';
import {useAutofocusRef} from 'shared/hooks';
import {unwrap} from 'shared/utils';

import './ConfirmSubmitStack.css';

export const confirmShouldSubmitEnabledAtom = atom<boolean>({
  key: 'confirmShouldSubmitEnabledAtom',
  default: true,
  effects: [persistAtomToConfigEffect('isl.show-stack-submit-confirmation', true as boolean)],
});

export type SubmitConfirmationReponse =
  | {submitAsDraft: boolean; updateMessage?: string}
  | undefined;

type SubmitType = 'submit' | 'submit-all' | 'resubmit';

export function shouldShowSubmitStackConfirmation(snapshot: Snapshot): boolean {
  const provider = snapshot.getLoadable(codeReviewProvider).valueMaybe();
  const shouldShowConfirmation = snapshot.getLoadable(confirmShouldSubmitEnabledAtom).valueMaybe();
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

  useRecoilValue(confirmShouldSubmitEnabledAtom); // ensure this config is loaded ahead of clicking this

  return useRecoilCallback(({snapshot}) => async (mode: SubmitType, stack: Array<CommitInfo>) => {
    if (!shouldShowSubmitStackConfirmation(snapshot)) {
      const draft = snapshot.getLoadable(submitAsDraft).valueMaybe();
      return {submitAsDraft: draft ?? false};
    }

    const provider = snapshot.getLoadable(codeReviewProvider).valueMaybe();

    const replace = {$numCommits: String(stack.length), $cmd: unwrap(provider).submitCommandName()};
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
  });
}

function ConfirmModalContent({
  stack,
  returnResultAndDismiss,
}: {
  stack: Array<CommitInfo>;
  returnResultAndDismiss: (value: SubmitConfirmationReponse) => unknown;
}) {
  const [showSubmitConfirmation, setShowSubmitConfirmation] = useRecoilState(
    confirmShouldSubmitEnabledAtom,
  );
  const shouldSubmitAsDraft = useRecoilValue(submitAsDraft);
  const [updateMessage, setUpdateMessage] = useState('');
  const commitsWithDiffs = stack.filter(commit => commit.diffId != null);

  const submitRef = useAutofocusRef();

  const provider = useRecoilValue(codeReviewProvider);
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
          <VSCodeTextField
            value={updateMessage}
            data-testid="submit-update-message-input"
            onChange={e => setUpdateMessage((e.target as HTMLInputElement).value)}>
            Update Message
          </VSCodeTextField>
        )}
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
          <VSCodeCheckbox
            checked={!showSubmitConfirmation}
            onChange={e => setShowSubmitConfirmation(!(e.target as HTMLInputElement).checked)}>
            <T>Don't show again</T>
          </VSCodeCheckbox>
        </Tooltip>
        <FlexSpacer />
        <VSCodeButton appearance="secondary" onClick={() => returnResultAndDismiss(undefined)}>
          <T>Cancel</T>
        </VSCodeButton>
        <VSCodeButton
          ref={submitRef as MutableRefObject<null>}
          appearance="primary"
          onClick={() =>
            returnResultAndDismiss({
              submitAsDraft: shouldSubmitAsDraft,
              updateMessage: updateMessage || undefined,
            })
          }>
          <T>Submit</T>
        </VSCodeButton>
      </div>
    </div>
  );
}
