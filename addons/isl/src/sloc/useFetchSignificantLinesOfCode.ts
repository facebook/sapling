/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFile, CommitInfo, Hash} from '../types';

import serverAPI from '../ClientToServerAPI';
import {commitInfoViewCurrentCommits} from '../CommitInfoView/CommitInfoState';
import {getGeneratedFilesFrom, useGeneratedFileStatuses} from '../GeneratedFile';
import {tracker} from '../analytics';
import {atomFamilyWeak, lazyAtom} from '../jotaiUtils';
import {GeneratedStatus} from '../types';
import {MAX_FETCHED_FILES_PER_COMMIT} from 'isl-server/src/commands';
import {useAtomValue} from 'jotai';
import {useState, useEffect} from 'react';

const commitSloc = atomFamilyWeak((hash: string) => {
  return lazyAtom(async get => {
    const commits = get(commitInfoViewCurrentCommits);
    if (commits == null || commits.length > 1) {
      return undefined;
    }
    const [commit] = commits;
    if (commit.totalFileCount > MAX_FETCHED_FILES_PER_COMMIT) {
      return undefined;
    }
    const filesToQueryGeneratedStatus = commit.filesSample.map(f => f.path);
    const generatedStatuses = getGeneratedFilesFrom(filesToQueryGeneratedStatus);

    const excludedFiles = filesToQueryGeneratedStatus.reduce<string[]>((filtered, path) => {
      // the __generated__ pattern is included in the exclusions, so we don't need to include it here
      if (!path.match(/__generated__/) && generatedStatuses[path] !== GeneratedStatus.Manual) {
        filtered.push(path);
      }
      return filtered;
    }, []);
    serverAPI.postMessage({
      type: 'fetchSignificantLinesOfCode',
      hash,
      excludedFiles,
    });

    const loc = await serverAPI
      .nextMessageMatching('fetchedSignificantLinesOfCode', message => message.hash === hash)
      .then(result => result.linesOfCode);

    return loc.value;
  }, undefined);
});

function fetchPendingSignificantLinesOfCode(hash: Hash, includedFiles: string[]) {
  // since pending changes can change, we aren't using a cache here to ensure the data is always current
  serverAPI.postMessage({
    type: 'fetchPendingSignificantLinesOfCode',
    hash,
    includedFiles,
  });

  return serverAPI
    .nextMessageMatching(
      'fetchedPendingSignificantLinesOfCode',
      message => message.type === 'fetchedPendingSignificantLinesOfCode' && message.hash === hash,
    )
    .then(result => result.linesOfCode);
}

export function useFetchSignificantLinesOfCode(commit: CommitInfo) {
  const significantLinesOfCode = useAtomValue(commitSloc(commit.hash));
  return significantLinesOfCode;
}

export function useFetchPendingSignificantLinesOfCode(
  commit: CommitInfo,
  selectedFiles: ChangedFile[],
) {
  const filesToQueryGeneratedStatus = selectedFiles.map(f => f.path);
  const generatedStatuses = useGeneratedFileStatuses(filesToQueryGeneratedStatus);

  const [significantLinesOfCode, setSignificantLinesOfCode] = useState<number | undefined>(
    undefined,
  );
  useEffect(() => {
    if (
      commit.totalFileCount > MAX_FETCHED_FILES_PER_COMMIT ||
      selectedFiles.length > MAX_FETCHED_FILES_PER_COMMIT
    ) {
      setSignificantLinesOfCode(undefined);
      return;
    }
    if (selectedFiles.length === 0) {
      setSignificantLinesOfCode(0);
      return;
    }
    const includedFiles = selectedFiles.reduce<string[]>((filtered, f) => {
      //only include non generated files
      if (generatedStatuses[f.path] === GeneratedStatus.Manual) {
        filtered.push(f.path);
      }
      return filtered;
    }, []);
    fetchPendingSignificantLinesOfCode(commit.hash, includedFiles).then(result => {
      if (result.error != null) {
        tracker.error('FetchPendingSloc', 'FetchError', result.error, {
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
  }, [selectedFiles, commit.hash, generatedStatuses, commit.totalFileCount]);

  return significantLinesOfCode;
}
