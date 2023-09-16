/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../types';

import {Tooltip} from '../Tooltip';
import {t, T} from '../i18n';
import {persistAtomToConfigEffect} from '../persistAtomToConfigEffect';
import {codeReviewProvider} from './CodeReviewInfo';
import {VSCodeCheckbox} from '@vscode/webview-ui-toolkit/react';
import {atom, useRecoilState, useRecoilValue} from 'recoil';

export const submitAsDraft = atom<boolean>({
  key: 'submitAsDraft',
  default: false,
  effects: [persistAtomToConfigEffect('isl.submitAsDraft')],
});

export function SubmitAsDraftCheckbox({
  commitsToBeSubmit,
  forceShow,
}:
  | {commitsToBeSubmit: Array<CommitInfo>; forceShow?: undefined}
  | {forceShow: true; commitsToBeSubmit?: undefined}) {
  const [isDraft, setIsDraft] = useRecoilState(submitAsDraft);
  const provider = useRecoilValue(codeReviewProvider);
  if (
    !forceShow &&
    (provider == null ||
      (provider?.supportSubmittingAsDraft === 'newDiffsOnly' &&
        // empty array => commit to submit is not yet created (this counts as a new Diff)
        commitsToBeSubmit.length > 0 &&
        // some commits don't have a diff ID => those are "new" Diffs
        commitsToBeSubmit.some(commit => commit.diffId != null)))
  ) {
    // hide draft button for diffs being resubmitted, if the provider doesn't support drafts on resubmission
    return null;
  }
  return (
    <VSCodeCheckbox
      className="submit-as-draft-checkbox"
      checked={isDraft}
      onChange={e => setIsDraft((e.target as HTMLInputElement).checked)}>
      <Tooltip
        title={
          forceShow
            ? t('Whether to submit diffs as drafts')
            : t('whetherToSubmitDiffAsDraft', {
                // we don't actually support submitting zero commits, instead this means we're submitting the head commit.
                count: commitsToBeSubmit?.length || 1,
              })
        }>
        <T>Submit as Draft</T>
      </Tooltip>
    </VSCodeCheckbox>
  );
}
