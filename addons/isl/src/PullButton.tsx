/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {t, T} from './i18n';
import {PullOperation} from './operations/PullOperation';
import {relativeDate, RelativeDate} from './relativeDate';
import {latestCommitTree, operationList, queuedOperations, useRunOperation} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';

import './PullButton.css';

export function PullButton() {
  const list = useRecoilValue(operationList);
  const queued = useRecoilValue(queuedOperations);
  const runOperation = useRunOperation();
  // no need to use previews here, we only need the latest commits to find the last pull timestamp.
  const latestCommits = useRecoilValue(latestCommitTree);
  // assuming master is getting updated frequently, last pull time should equal the newest commit in the history.
  const lastSync = Math.max(...latestCommits.map(commit => commit.info.date.valueOf()));

  let title =
    t('Fetch latest repository and branch information from remote.') +
    '\n\n' +
    t('Last synced with remote:') +
    ' ' +
    relativeDate(lastSync, {});
  let inProgress = false;

  if (
    list.currentOperation?.operation instanceof PullOperation &&
    list.currentOperation?.exitCode == null
  ) {
    inProgress = true;
    title += '\n\n' + t('Pull is currently running.');
  } else if (queued.some(op => op instanceof PullOperation)) {
    inProgress = true;
    title += '\n\n' + t('Pull is already scheduled.');
  }

  return (
    <Tooltip placement="bottom" delayMs={DOCUMENTATION_DELAY} title={title}>
      <div className="pull-info">
        <VSCodeButton
          appearance="secondary"
          disabled={inProgress}
          onClick={() => {
            runOperation(new PullOperation());
          }}>
          <Icon slot="start" icon={inProgress ? 'loading' : 'cloud-download'} />
          <T>Pull</T>
        </VSCodeButton>
        <RelativeDate date={lastSync} useShortVariant />
      </div>
    </Tooltip>
  );
}
