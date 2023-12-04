/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {codeReviewProvider, diffSummary} from './codeReview/CodeReviewInfo';
import {t, T} from './i18n';
import {UncommitOperation} from './operations/Uncommit';
import foundPlatform from './platform';
import {dagWithPreviews} from './previews';
import {useRunOperation} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';

export function UncommitButton() {
  const dag = useRecoilValue(dagWithPreviews);
  const headCommit = dag.resolve('.');

  const provider = useRecoilValue(codeReviewProvider);
  const diff = useRecoilValue(diffSummary(headCommit?.diffId));
  const isClosed = provider != null && diff.value != null && provider?.isDiffClosed(diff.value);

  const runOperation = useRunOperation();
  if (!headCommit || dag.children(headCommit?.hash).size > 0) {
    // if the head commit has children, we can't uncommit
    return null;
  }

  if (isClosed) {
    return null;
  }
  return (
    <Tooltip
      delayMs={DOCUMENTATION_DELAY}
      title={t(
        'Remove this commit, but keep its changes as uncommitted changes, as if you never ran commit.',
      )}>
      <VSCodeButton
        onClick={async () => {
          const confirmed = await foundPlatform.confirm(
            t('Are you sure you want to Uncommit?'),
            t(
              'Uncommitting will remove this commit, but keep its changes as uncommitted changes, as if you never ran commit.',
            ),
          );
          if (!confirmed) {
            return;
          }
          runOperation(new UncommitOperation(headCommit));
        }}
        appearance="icon">
        <Icon icon="debug-step-out" slot="start" />
        <T>Uncommit</T>
      </VSCodeButton>
    </Tooltip>
  );
}
