/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {BookmarksManagerMenu} from './BookmarksManager';
import {BugButton} from './BugButton';
import {BulkActionsMenu} from './BulkActionsMenu';
import serverAPI from './ClientToServerAPI';
import {CwdSelector} from './CwdSelector';
import {DownloadCommitsTooltipButton} from './DownloadCommitsMenu';
import {FocusModeToggle} from './FocusMode';
import {genereatedFileCache} from './GeneratedFile';
import {PullButton} from './PullButton';
import {SettingsGearButton} from './SettingsTooltip';
import {ShelvedChangesMenu} from './ShelvedChanges';
import {tracker} from './analytics';
import {DebugToolsButton} from './debug/DebugToolsButton';
import {t} from './i18n';
import {maybeRemoveForgottenOperation, useClearAllOptimisticState} from './operationsState';
import {haveCommitsLoadedYet, haveRemotePath, isFetchingCommits} from './serverAPIState';
import {Button} from 'isl-components/Button';
import {FlexSpacer} from 'isl-components/Flex';
import {Icon} from 'isl-components/Icon';
import {DOCUMENTATION_DELAY, Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {clearTrackedCache} from 'shared/LRU';

import './TopBar.css';

export function TopBar() {
  const loaded = useAtomValue(haveCommitsLoadedYet);
  const canPush = useAtomValue(haveRemotePath);
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
        <BookmarksManagerMenu />
        <FetchingDataIndicator />
      </span>
      <span className="button-group">
        <FlexSpacer />
        <DebugToolsButton />
        <FocusModeToggle />
        <BugButton />
        <SettingsGearButton />
        <RefreshButton />
      </span>
    </div>
  );
}

function FetchingDataIndicator() {
  const isFetching = useAtomValue(isFetchingCommits);
  return <Icon icon={isFetching ? 'loading' : 'blank'} />;
}

function RefreshButton() {
  const clearOptimisticState = useClearAllOptimisticState();
  return (
    <Tooltip
      delayMs={DOCUMENTATION_DELAY}
      placement="bottom"
      title={t('Re-fetch latest commits and uncommitted changes.')}>
      <Button
        onClick={() => {
          tracker.track('ClickedRefresh');
          clearOptimisticState();
          maybeRemoveForgottenOperation();
          genereatedFileCache.clear(); // allow generated files to be rechecked
          serverAPI.postMessage({type: 'refresh'});
          clearTrackedCache();
        }}
        data-testid="refresh-button">
        <Icon icon="refresh" />
      </Button>
    </Tooltip>
  );
}
