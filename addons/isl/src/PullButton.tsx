/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Icon} from './Icon';
import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {t, T} from './i18n';
import {PullOperation} from './operations/PullOperation';
import {relativeDate, RelativeDate} from './relativeDate';
import {isFetchingCommits, latestCommitTree, useRunOperation} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';

import './PullButton.css';

export function PullButton() {
  const runOperation = useRunOperation();
  // no need to use previews here, we only need the latest commits to find the last pull timestamp.
  const latestCommits = useRecoilValue(latestCommitTree);
  const isFetching = useRecoilValue(isFetchingCommits);
  // assuming master is getting updated frequently, last pull time should equal the newest commit in the history.
  const lastSync = Math.max(...latestCommits.map(commit => commit.info.date.valueOf()));
  return (
    <Tooltip
      placement="bottom"
      delayMs={DOCUMENTATION_DELAY}
      title={
        t('Fetch latest repository and branch information from remote.') +
        '\n\n' +
        t('Last synced with remote:') +
        ' ' +
        relativeDate(lastSync, {})
      }>
      <div className="pull-info">
        <VSCodeButton
          appearance="secondary"
          onClick={() => {
            runOperation(new PullOperation());
          }}>
          <Icon slot="start" icon="cloud-download" />
          <T>Pull</T>
        </VSCodeButton>
        <RelativeDate date={lastSync} useShortVariant />
        {isFetching ? <Icon icon="loading" /> : null}
      </div>
    </Tooltip>
  );
}
