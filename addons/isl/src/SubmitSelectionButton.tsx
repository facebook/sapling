/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from './types';

import {HighlightCommitsWhileHovering} from './HighlightedCommits';
import {OperationDisabledButton} from './OperationDisabledButton';
import {multiSubmitUpdateMessage} from './SubmitUpdateMessageInput';
import {Tooltip} from './Tooltip';
import {allDiffSummaries, codeReviewProvider} from './codeReview/CodeReviewInfo';
import {submitAsDraft} from './codeReview/DraftCheckbox';
import {t, T} from './i18n';
import {readAtom, writeAtom} from './jotaiUtils';
import {dagWithPreviews} from './previews';
import {selectedCommits} from './selection';
import {atom, useAtomValue} from 'jotai';

/**
 * If the selected commits are submittable by the review provider,
 * they may be submit.
 */
export const submittableSelection = atom(get => {
  const selection = get(selectedCommits);
  if (selection.size < 2) {
    return undefined;
  }
  const provider = get(codeReviewProvider);
  const diffSummaries = get(allDiffSummaries);

  if (provider == null || diffSummaries == null) {
    return undefined;
  }

  const dag = get(dagWithPreviews);
  const commits = dag.getBatch(dag.sortAsc(selection));
  const submittable =
    (diffSummaries.value != null
      ? provider?.getSubmittableDiffs(commits, diffSummaries.value)
      : undefined) ?? [];

  return submittable;
});

/**
 * Button to submit the selected commits, if applicable.
 * If `commit` is provided, only render the button if the commit is the bottom of the selected range.
 * If `commit` is null, always show the button if there are commits to submit.
 */
export function SubmitSelectionButton({commit}: {commit?: CommitInfo}) {
  const submittable = useAtomValue(submittableSelection);
  const provider = useAtomValue(codeReviewProvider);

  if (
    provider == null ||
    submittable == null ||
    submittable.length < 2 ||
    // show the button on the bottom commit of the submittable selection, if showing the button on a commit.
    (commit != null && submittable?.[0]?.hash !== commit.hash)
  ) {
    return null;
  }

  return (
    <Tooltip
      title={t('Submit $count selected commits for code review with $provider', {
        replace: {
          $count: String(submittable.length),
          $provider: provider.label ?? 'remote',
        },
      })}>
      <HighlightCommitsWhileHovering toHighlight={submittable}>
        <OperationDisabledButton
          appearance="secondary"
          runOperation={() => {
            const updateMessage = readAtom(multiSubmitUpdateMessage(submittable));
            // clear update message on submit
            writeAtom(multiSubmitUpdateMessage(submittable), '');
            return provider.submitOperation(submittable, {
              draft: readAtom(submitAsDraft),
              updateMessage: updateMessage || undefined,
            });
          }}
          contextKey={`submit-selection-${submittable[0].hash}`}>
          <T replace={{$count: submittable.length}}>Submit $count commits</T>
        </OperationDisabledButton>
      </HighlightCommitsWhileHovering>
    </Tooltip>
  );
}
