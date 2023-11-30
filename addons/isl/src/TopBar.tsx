/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {BugButton} from './BugButton';
import {BulkActionsMenu} from './BulkActionsMenu';
import serverAPI from './ClientToServerAPI';
import {CwdSelector} from './CwdSelector';
import {DownloadCommitsTooltipButton} from './DownloadCommitsMenu';
import {PullButton} from './PullButton';
import {SettingsGearButton} from './SettingsTooltip';
import {ShelvedChangesMenu} from './ShelvedChanges';
import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {tracker} from './analytics';
import {DebugToolsButton} from './debug/DebugToolsButton';
import {t} from './i18n';
import {
  haveCommitsLoadedYet,
  haveRemotePath,
  isFetchingCommits,
  useClearAllOptimisticState,
} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';

import './TopBar.css';

export function TopBar() {
  const loaded = useRecoilValue(haveCommitsLoadedYet);
  const canPush = useRecoilValue(haveRemotePath);
  if (!loaded) {
    return null;
  }
  return (
    <div className="top-bar">
      <span className="button-group">
        {canPush && <PullButton />}
        <CwdSelector />
        <DownloadCommitsTooltipButton />
        <ShelvedChangesMenu />
        <BulkActionsMenu />
        <FetchingDataIndicator />
      </span>
      <span className="button-group">
        <DebugToolsButton />
        <BugButton />
        <SettingsGearButton />
        <RefreshButton />
      </span>
    </div>
  );
}

function FetchingDataIndicator() {
  const isFetching = useRecoilValue(isFetchingCommits);
  return isFetching ? <Icon icon="loading" /> : null;
}

function RefreshButton() {
  const clearOptimisticState = useClearAllOptimisticState();
  return (
    <Tooltip
      delayMs={DOCUMENTATION_DELAY}
      placement="bottom"
      title={t('Re-fetch latest commits and uncommitted changes.')}>
      <VSCodeButton
        appearance="secondary"
        onClick={() => {
          tracker.track('ClickedRefresh');
          clearOptimisticState();
          serverAPI.postMessage({type: 'refresh'});
        }}
        data-testid="refresh-button">
        <Icon icon="refresh" />
      </VSCodeButton>
    </Tooltip>
  );
}
