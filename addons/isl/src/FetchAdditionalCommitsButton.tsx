/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import serverAPI from './ClientToServerAPI';
import {t, T} from './i18n';
import {writeAtom} from './jotaiUtils';
import {CommitCloudSyncOperation} from './operations/CommitCloudSyncOperation';
import {useRunOperation} from './operationsState';
import {useIsOperationRunningOrQueued} from './previews';
import {commitsShownRange, isFetchingAdditionalCommits} from './serverAPIState';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {DOCUMENTATION_DELAY, Tooltip} from 'isl-components/Tooltip';
import {atom, useAtomValue} from 'jotai';

export function FetchingAdditionalCommitsRow() {
  return (
    <div className="fetch-additional-commits-row">
      <FetchingAdditionalCommitsButton />
      <FetchingAdditionalCommitsIndicator />
    </div>
  );
}

const hasSyncedFromCloudAtom = atom(false);

function FetchingAdditionalCommitsIndicator() {
  const isFetching = useAtomValue(isFetchingAdditionalCommits);
  return isFetching ? <Icon icon="loading" /> : null;
}

function FetchingAdditionalCommitsButton() {
  const shownRange = useAtomValue(commitsShownRange);
  const isRunningSync = useIsOperationRunningOrQueued(CommitCloudSyncOperation) != null;
  const isFetching = useAtomValue(isFetchingAdditionalCommits);
  const isLoading = isFetching || isRunningSync;
  const hasAlreadySynced = useAtomValue(hasSyncedFromCloudAtom);
  const runOperation = useRunOperation();
  if (shownRange === undefined && hasAlreadySynced) {
    return null;
  }
  const fetchFromCloudNext = shownRange == null;
  const commitsShownMessage = fetchFromCloudNext
    ? t('Showing full commit history. Click to fetch all commits from Commit Cloud')
    : t('Showing commits from the last $numDays days', {
        replace: {$numDays: shownRange.toString()},
      });
  return (
    <Tooltip placement="top" delayMs={DOCUMENTATION_DELAY} title={commitsShownMessage}>
      <Button
        disabled={isLoading}
        onClick={() => {
          if (fetchFromCloudNext) {
            runOperation(new CommitCloudSyncOperation(/* full */ true)).then(() =>
              writeAtom(hasSyncedFromCloudAtom, true),
            );
            return;
          }

          serverAPI.postMessage({
            type: 'loadMoreCommits',
          });
        }}
        icon>
        {fetchFromCloudNext ? (
          <>
            <Icon icon={isLoading ? 'spinner' : 'cloud-download'} /> Fetch all cloud commits
          </>
        ) : (
          <T>Load more commits</T>
        )}
      </Button>
    </Tooltip>
  );
}
