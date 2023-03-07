/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {t, T} from './i18n';
import {PullOperation} from './operations/PullOperation';
import {useIsOperationRunningOrQueued} from './previews';
import {relativeDate, RelativeDate} from './relativeDate';
import {latestCommitTree, useRunOperation} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';

import './PullButton.css';

export function PullButton() {
  const runOperation = useRunOperation();
  // no need to use previews here, we only need the latest commits to find the last pull timestamp.
  const latestCommits = useRecoilValue(latestCommitTree);
  // assuming master is getting updated frequently, last pull time should equal the newest commit in the history.
  const lastSync =
    latestCommits.length === 0
      ? null
      : Math.max(...latestCommits.map(commit => commit.info.date.valueOf()));

  let title =
    t('Fetch latest repository and branch information from remote.') +
    '\n\n' +
    (lastSync == null
      ? ''
      : t('Last synced with remote: $date', {replace: {$date: relativeDate(lastSync, {})}}));

  const isRunningPull = useIsOperationRunningOrQueued(PullOperation);
  if (isRunningPull === 'queued') {
    title += '\n\n' + t('Pull is currently running.');
  } else if (isRunningPull === 'running') {
    title += '\n\n' + t('Pull is already scheduled.');
  }

  return (
    <Tooltip placement="bottom" delayMs={DOCUMENTATION_DELAY} title={title}>
      <div className="pull-info">
        <VSCodeButton
          appearance="secondary"
          disabled={!!isRunningPull}
          onClick={() => {
            runOperation(new PullOperation());
          }}>
          <Icon slot="start" icon={isRunningPull ? 'loading' : 'cloud-download'} />
          <T>Pull</T>
        </VSCodeButton>
        {lastSync && <RelativeDate date={lastSync} useShortVariant />}
      </div>
    </Tooltip>
  );
}
