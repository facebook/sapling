/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Banner, BannerKind} from '../Banner';
import serverAPI from '../ClientToServerAPI';
import {useGeneratedFileStatuses} from '../GeneratedFile';
import {Internal} from '../Internal';
import {Tooltip} from '../Tooltip';
import {tracker} from '../analytics';
import {Divider} from '../components/Divider';
import {useFeatureFlagSync} from '../featureFlags';
import {T} from '../i18n';
import {SplitButton} from '../stackEdit/ui/SplitButton';
import {GeneratedStatus, type CommitInfo, type Hash, type Result} from '../types';
import {useEffect, useState} from 'react';
import {Icon} from 'shared/Icon';
import {LRU} from 'shared/LRU';

// Cache fetches in progress so we don't double fetch
const commitFilesCache = new LRU<Hash, Promise<Result<number>>>(10);

function fetchSignificantLinesOfCode(hash: Hash, generatedFiles: string[]) {
  const foundPromise = commitFilesCache.get(hash);
  if (foundPromise != null) {
    return foundPromise;
  }
  serverAPI.postMessage({
    type: 'fetchSignificantLinesOfCode',
    hash,
    generatedFiles,
  });

  const resultPromise = serverAPI
    .nextMessageMatching('fetchedSignificantLinesOfCode', message => message.hash === hash)
    .then(result => result.linesOfCode);

  commitFilesCache.set(hash, resultPromise);

  return resultPromise;
}

function SplitSuggestionImpl({commit}: {commit: CommitInfo}) {
  const filesToQueryGeneratedStatus = commit.filesSample.map(f => f.path);
  const generatedStatuses = useGeneratedFileStatuses(filesToQueryGeneratedStatus);

  const [significantLinesOfCode, setSignificantLinesOfCode] = useState(0);
  useEffect(() => {
    const generatedFiles = commit.filesSample.reduce<string[]>((filtered, f) => {
      // the __generated__ pattern is included in the exclusions, so we don't need to include it here
      if (!f.path.match(/__generated__/) && generatedStatuses[f.path] !== GeneratedStatus.Manual) {
        filtered.push(f.path);
      }
      return filtered;
    }, []);
    fetchSignificantLinesOfCode(commit.hash, generatedFiles).then(result => {
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
  }, [commit.filesSample, commit.hash, generatedStatuses]);
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
            trackerEventName="SplitOpenFromSplitSuggestion"
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

function GatedSplitSuggestion({commit}: {commit: CommitInfo}) {
  const showSplitSuggestion = useFeatureFlagSync(Internal.featureFlags?.ShowSplitSuggestion);

  if (!showSplitSuggestion) {
    return null;
  }
  return <SplitSuggestionImpl commit={commit} />;
}

export default function SplitSuggestion({commit}: {commit: CommitInfo}) {
  if (commit.totalFileCount > 25) {
    return null;
  }
  // using a gated component to avoid exposing when diff size is too big  to show the split suggestion
  return <GatedSplitSuggestion commit={commit} />;
}
