/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {codeReviewProvider, diffSummary} from './codeReview/CodeReviewInfo';
import {t, T} from './i18n';
import {UncommitOperation} from './operations/Uncommit';
import {useRunOperation} from './operationsState';
import foundPlatform from './platform';
import {dagWithPreviews} from './previews';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {DOCUMENTATION_DELAY, Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';

export function UncommitButton() {
  const dag = useAtomValue(dagWithPreviews);
  const headCommit = dag.resolve('.');

  const provider = useAtomValue(codeReviewProvider);
  const diff = useAtomValue(diffSummary(headCommit?.diffId));
  const isClosed = provider != null && diff.value != null && provider?.isDiffClosed(diff.value);

  const runOperation = useRunOperation();
  if (!headCommit) {
    return null;
  }

  const hasChildren = dag.children(headCommit?.hash).size > 0;

  if (isClosed) {
    return null;
  }
  return (
    <Tooltip
      delayMs={DOCUMENTATION_DELAY}
      title={
        hasChildren
          ? t(
              'Go back to the previous commit, but keep the changes by skipping updating files in the working copy. Note: the original commit will not be deleted because it has children.',
            )
          : t(
              'Hide this commit, but keep its changes as uncommitted changes, as if you never ran commit.',
            )
      }>
      <Button
        onClick={async e => {
          e.stopPropagation();
          const confirmed = await foundPlatform.confirm(
            t('Are you sure you want to Uncommit?'),
            hasChildren
              ? t(
                  'Uncommitting will not hide the original commit because it has children, but will move to the parent commit and keep its changes as uncommitted changes.',
                )
              : t(
                  'Uncommitting will hide this commit, but keep its changes as uncommitted changes, as if you never ran commit.',
                ),
          );
          if (!confirmed) {
            return;
          }
          runOperation(new UncommitOperation(headCommit));
        }}
        icon
        data-testid="uncommit-button">
        <Icon icon="debug-step-out" slot="start" />
        <T>Uncommit</T>
      </Button>
    </Tooltip>
  );
}
