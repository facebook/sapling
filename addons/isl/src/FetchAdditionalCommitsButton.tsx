/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {DOCUMENTATION_DELAY, Tooltip} from 'isl-components/Tooltip';
import {atom, useAtomValue} from 'jotai';
import {Suspense} from 'react';
import serverAPI from './ClientToServerAPI';
import {commitCloudEnabledAtom} from './CommitCloud';
import {t, T} from './i18n';
import {writeAtom} from './jotaiUtils';
import {CommitCloudSyncOperation} from './operations/CommitCloudSyncOperation';
import {useRunOperation} from './operationsState';
import {useIsOperationRunningOrQueued} from './previews';
import {commitsShownRange, isFetchingAdditionalCommits} from './serverAPIState';

export function FetchingAdditionalCommitsRow() {
  return (
    <Suspense>
      <div className="fetch-additional-commits-row">
        <FetchingAdditionalCommitsButton />
        <FetchingAdditionalCommitsIndicator />
      </div>
    </Suspense>
  );
}

const hasSyncedFromCloudAtom = atom(false);

function FetchingAdditionalCommitsIndicator() {
  const isFetching = useAtomValue(isFetchingAdditionalCommits);
  return isFetching ? <Icon icon="loading" /> : null;
}

function FetchingAdditionalCommitsButton() {
  const shownRange = useAtomValue(commitsShownRange);
  const isLoading = useAtomValue(isFetchingAdditionalCommits);
  const hasAlreadySynced = useAtomValue(hasSyncedFromCloudAtom);
  if (shownRange === undefined && hasAlreadySynced) {
    return null;
  }
  const fetchFromCloudNext = shownRange == null;
  if (fetchFromCloudNext) {
    return <LoadMoreFromCloudButton />;
  }
  const commitsShownMessage = t('Showing commits from the last $numDays days', {
    replace: {$numDays: shownRange.toString()},
  });
  return (
    <Tooltip placement="top" delayMs={DOCUMENTATION_DELAY} title={commitsShownMessage}>
      <Button
        disabled={isLoading}
        onClick={() => {
          serverAPI.postMessage({
            type: 'loadMoreCommits',
          });
        }}
        icon>
        <T>Load more commits</T>
      </Button>
    </Tooltip>
  );
}

function LoadMoreFromCloudButton() {
  const runOperation = useRunOperation();
  const isRunning = useIsOperationRunningOrQueued(CommitCloudSyncOperation) != null;
  const isFetching = useAtomValue(isFetchingAdditionalCommits);
  const isLoading = isRunning || isFetching;
  const isCloudEnabled = useAtomValue(commitCloudEnabledAtom);
  if (!isCloudEnabled) {
    return null;
  }
  return (
    <Tooltip
      placement="top"
      delayMs={DOCUMENTATION_DELAY}
      title={t('Showing full commit history. Click to fetch all commits from Commit Cloud')}>
      <Button
        disabled={isLoading}
        onClick={() => {
          runOperation(new CommitCloudSyncOperation(/* full */ true)).then(() =>
            writeAtom(hasSyncedFromCloudAtom, true),
          );
        }}
        icon>
        <Icon icon={isLoading ? 'spinner' : 'cloud-download'} /> Fetch all cloud commits
      </Button>
    </Tooltip>
  );
}
