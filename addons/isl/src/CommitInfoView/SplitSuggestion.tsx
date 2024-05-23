/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, Hash, Result} from '../types';

import {Banner, BannerKind} from '../Banner';
import serverAPI from '../ClientToServerAPI';
import {Internal} from '../Internal';
import {Tooltip} from '../Tooltip';
import {tracker} from '../analytics';
import {Divider} from '../components/Divider';
import {useFeatureFlagSync} from '../featureFlags';
import {T} from '../i18n';
import {SplitButton} from '../stackEdit/ui/SplitButton';
import {useEffect, useState} from 'react';
import {Icon} from 'shared/Icon';
import {LRU} from 'shared/LRU';

// Cache fetches in progress so we don't double fetch
const commitFilesCache = new LRU<Hash, Promise<Result<number>>>(10);

function fetchSignificantLinesOfCode(hash: Hash) {
  const foundPromise = commitFilesCache.get(hash);
  if (foundPromise != null) {
    return foundPromise;
  }
  serverAPI.postMessage({
    type: 'fetchSignificantLinesOfCode',
    hash,
  });

  const resultPromise = serverAPI
    .nextMessageMatching('fetchedSignificantLinesOfCode', message => message.hash === hash)
    .then(result => result.linesOfCode);

  commitFilesCache.set(hash, resultPromise);

  return resultPromise;
}

function SplitSuggestionImpl({commit}: {commit: CommitInfo}) {
  const [significantLinesOfCode, setSignificantLinesOfCode] = useState(0);
  useEffect(() => {
    fetchSignificantLinesOfCode(commit.hash).then(result => {
      if (result.error != null) {
        tracker.error('SplitSuggestionError', 'SplitSuggestionError', result.error, {
          extras: {
            commitHash: commit.hash,
          },
        });
        return;
      }
      if (result.value != null) {
        setSignificantLinesOfCode(result.value);
      }
    });
  }, [commit.hash]);
  if (significantLinesOfCode <= 100) {
    return null;
  }
  return (
    <>
      <Divider />
      <Banner
        tooltip=""
        kind={BannerKind.green}
        icon={<Icon icon="info" />}
        alwaysShowButtons
        buttons={
          <SplitButton
            style={{
              border: '1px solid var(--button-secondary-hover-background)',
            }}
            commit={commit}
          />
        }>
        <div>
          <T>Pro tip: Small Diffs lead to less SEVs, quicker review times and happier teams.</T>
          &nbsp;
          <Tooltip
            inline={true}
            trigger="hover"
            title={`Significant Lines of Code (SLOC): ${significantLinesOfCode}, this puts your diff in the top 10% of diffs. `}>
            <T>This diff is a bit big</T>
          </Tooltip>
          <T>, consider splitting it up</T>
        </div>
      </Banner>
    </>
  );
}

export default function SplitSuggestion({commit}: {commit: CommitInfo}) {
  const showSplitSuggestion = useFeatureFlagSync(Internal.featureFlags?.ShowSplitSuggestion);

  if (!showSplitSuggestion) {
    return null;
  }
  return <SplitSuggestionImpl commit={commit} />;
}
