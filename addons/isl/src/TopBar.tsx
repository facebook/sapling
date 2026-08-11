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
import {useCallback, useRef} from 'react';
import {clearTrackedCache} from 'shared/LRU';
import {BookmarksManagerMenu} from './BookmarksManager';
import {BugButton} from './BugButton';
import {BulkActionsMenu} from './BulkActionsMenu';
import serverAPI from './ClientToServerAPI';
import {scrollToYouAreHere} from './CommitTreeList';
import {CommitTreeSearchFilterButton} from './CommitTreeSearchFilter';
import {CwdSelector} from './CwdSelector';
import {DownloadCommitsTooltipButton} from './DownloadCommitsMenu';
import {FocusModeToggle} from './FocusMode';
import {generatedFileCache} from './GeneratedFile';
import {PullButton} from './PullButton';
import {SettingsGearButton} from './SettingsTooltip';
import {ShelvedChangesMenu} from './ShelvedChanges';
import {tracker} from './analytics';
import {DebugToolsButton} from './debug/DebugToolsButton';
import {T} from './i18n';
import {maybeRemoveForgottenOperation, useClearAllOptimisticState} from './operationsState';
import {haveCommitsLoadedYet, haveRemotePath, isFetchingCommits} from './serverAPIState';

import {Internal} from './Internal';
import './TopBar.css';

// Publish the (wrapping, variable-height) top bar's height as a CSS var so overlays like
// the floating "scroll to current commit" pill can sit below it instead of over it.
// A ref callback (not useEffect) because the bar mounts only once commits have loaded.
function useTopBarHeightVar() {
  const observerRef = useRef<ResizeObserver | null>(null);
  return useCallback((el: HTMLDivElement | null) => {
    observerRef.current?.disconnect();
    if (el == null) {
      document.body.style.removeProperty('--top-bar-height');
      return;
    }
    document.body.style.setProperty('--top-bar-height', `${el.offsetHeight}px`);
    const obs = new ResizeObserver(([entry]) => {
      const height = entry.borderBoxSize?.[0]?.blockSize ?? el.offsetHeight;
      document.body.style.setProperty('--top-bar-height', `${height}px`);
    });
    obs.observe(el);
    observerRef.current = obs;
  }, []);
}

export function TopBar() {
  const loaded = useAtomValue(haveCommitsLoadedYet);
  const canPush = useAtomValue(haveRemotePath);
  const ref = useTopBarHeightVar();

  if (!loaded) {
    return null;
  }
  return (
    <div className="top-bar" ref={ref}>
      <span className="button-group">
        {canPush && <PullButton />}
        <CwdSelector />
        <DownloadCommitsTooltipButton />
        <ShelvedChangesMenu />
        <BulkActionsMenu />
        <BookmarksManagerMenu />
        {Internal.FullRepoBranchButton && <Internal.FullRepoBranchButton />}
        <FetchingDataIndicator />
      </span>
      <span className="button-group">
        <DebugToolsButton />
        <ScrollToYouAreHereButton />
        <CommitTreeSearchFilterButton />
        <FocusModeToggle />
        <BugButton />
        <SettingsGearButton />
        <RefreshButton />
      </span>
    </div>
  );
}

function ScrollToYouAreHereButton() {
  return (
    <Tooltip
      delayMs={DOCUMENTATION_DELAY}
      placement="bottom"
      title={<T>Scroll to your current commit ("You are here")</T>}>
      <Button icon onClick={() => scrollToYouAreHere()} data-testid="scroll-to-you-are-here-button">
        <Icon icon="target" />
      </Button>
    </Tooltip>
  );
}

function FetchingDataIndicator() {
  const isFetching = useAtomValue(isFetchingCommits);
  return (
    <span className="fetching-data-indicator">{isFetching ? <Icon icon="loading" /> : null}</span>
  );
}

function RefreshButton() {
  const clearOptimisticState = useClearAllOptimisticState();
  return (
    <Tooltip
      delayMs={DOCUMENTATION_DELAY}
      placement="bottom"
      title={<T>Re-fetch latest commits and uncommitted changes.</T>}>
      <Button
        onClick={() => {
          tracker.track('ClickedRefresh');
          clearOptimisticState();
          maybeRemoveForgottenOperation();
          generatedFileCache.clear(); // allow generated files to be rechecked
          serverAPI.postMessage({type: 'refresh'});
          clearTrackedCache();
        }}
        data-testid="refresh-button">
        <Icon icon="refresh" />
      </Button>
    </Tooltip>
  );
}
