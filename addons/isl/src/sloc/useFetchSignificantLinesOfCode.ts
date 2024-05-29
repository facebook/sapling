/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, Hash, Result} from '../types';

import serverAPI from '../ClientToServerAPI';
import {useGeneratedFileStatuses} from '../GeneratedFile';
import {tracker} from '../analytics';
import {GeneratedStatus} from '../types';
import {useState, useEffect} from 'react';
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

export function useFetchSignificantLinesOfCode(commit: CommitInfo) {
  const filesToQueryGeneratedStatus = commit.filesSample.map(f => f.path);
  const generatedStatuses = useGeneratedFileStatuses(filesToQueryGeneratedStatus);

  const [significantLinesOfCode, setSignificantLinesOfCode] = useState<number | undefined>(
    undefined,
  );
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

  return significantLinesOfCode;
}
