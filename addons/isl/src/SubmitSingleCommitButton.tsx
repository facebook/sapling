/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {DOCUMENTATION_DELAY, Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {tracker} from './analytics';
import {codeReviewProvider, diffSummary} from './codeReview/CodeReviewInfo';
import {submitAsDraft} from './codeReview/DraftCheckbox';
import {publishWhenReady} from './codeReview/PublishWhenReadyCheckbox';
import {t, T} from './i18n';
import {readAtom} from './jotaiUtils';
import {useRunOperation} from './operationsState';
import {dagWithPreviews} from './previews';

export function SubmitSingleCommitButton() {
  const dag = useAtomValue(dagWithPreviews);
  const headCommit = dag.resolve('.');

  const provider = useAtomValue(codeReviewProvider);
  const diff = useAtomValue(diffSummary(headCommit?.diffId));
  const isClosed = provider != null && diff.value != null && provider?.isDiffClosed(diff.value);

  const runOperation = useRunOperation();

  if (!headCommit || !provider) {
    return null;
  }

  const draftAncestors = dag.ancestors(headCommit.hash, {within: dag.draft()});
  const isSingleCommit = draftAncestors.size === 1;
  const hasDiff = headCommit.diffId !== undefined;

  if (!isSingleCommit || isClosed || hasDiff) {
    return null;
  }

  const tooltip = t('Submit this commit for review with $cmd.', {
    replace: {$cmd: provider.submitCommandName()},
  });

  return (
    <Tooltip delayMs={DOCUMENTATION_DELAY} title={tooltip}>
      <Button
        onClick={e => {
          e.stopPropagation();
          tracker.track('SubmitSingleCommit');
          const draft = readAtom(submitAsDraft);
          runOperation(
            provider.submitOperation([], {
              draft: draft ?? false,
              publishWhenReady: readAtom(publishWhenReady),
            }),
          );
        }}
        icon
        data-testid="submit-button">
        <Icon icon="cloud-upload" slot="start" />
        <T>Submit</T>
      </Button>
    </Tooltip>
  );
}
