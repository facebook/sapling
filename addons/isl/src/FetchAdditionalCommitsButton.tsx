/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import serverAPI from './ClientToServerAPI';
import {t, T} from './i18n';
import {commitsShownRange, isFetchingAdditionalCommits} from './serverAPIState';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {DOCUMENTATION_DELAY, Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';

export function FetchingAdditionalCommitsRow() {
  return (
    <div className="fetch-additional-commits-row">
      <FetchingAdditionalCommitsButton />
      <FetchingAdditionalCommitsIndicator />
    </div>
  );
}

function FetchingAdditionalCommitsIndicator() {
  const isFetching = useAtomValue(isFetchingAdditionalCommits);
  return isFetching ? <Icon icon="loading" /> : null;
}

function FetchingAdditionalCommitsButton() {
  const shownRange = useAtomValue(commitsShownRange);
  const isFetching = useAtomValue(isFetchingAdditionalCommits);
  if (shownRange === undefined) {
    return null;
  }
  const commitsShownMessage = t('Showing commits from the last $numDays days', {
    replace: {$numDays: shownRange.toString()},
  });
  return (
    <Tooltip placement="top" delayMs={DOCUMENTATION_DELAY} title={commitsShownMessage}>
      <Button
        disabled={isFetching}
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
