/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash, Result, ChangedFile, CommitInfo} from './types';

import serverAPI from './ClientToServerAPI';
import {ChangedFiles} from './UncommittedChanges';
import {useState, useEffect} from 'react';
import {ComparisonType} from 'shared/Comparison';
import {LRU} from 'shared/LRU';

// Cache fetches in progress so we don't double fetch
const allCommitFilesCache = new LRU<Hash, Promise<Result<Array<ChangedFile>>>>(10);

/**
 * The basic CommitInfo we fetch in bulk only contains the first 25 files.
 * But we want to be able to scroll through pages of files.
 * So fetch all files for the currently selected commit,
 * to augment the subset we already have.
 */
export function ChangedFilesWithFetching({commit}: {commit: CommitInfo}) {
  const [fetchedAllFiles, setFetchedAllFiles] = useState<Array<ChangedFile> | undefined>(undefined);

  const hasAllFilesAlready = commit.filesSample.length === commit.totalFileCount;
  useEffect(() => {
    setFetchedAllFiles(undefined);
    if (hasAllFilesAlready) {
      return;
    }
    getChangedFilesForHash(commit.hash).then(result => {
      if (result.value != null) {
        setFetchedAllFiles(result.value);
      }
    });
  }, [commit.hash, hasAllFilesAlready]);

  return (
    <ChangedFiles
      filesSubset={fetchedAllFiles ?? commit.filesSample}
      totalFiles={commit.totalFileCount}
      comparison={
        commit.isDot
          ? {type: ComparisonType.HeadChanges}
          : {
              type: ComparisonType.Committed,
              hash: commit.hash,
            }
      }
    />
  );
}

/** Get all the changed files in a given commit.
 * A subset of the files may have already been fetched,
 * or in some cases no files may be cached yet and all files need to be fetched asynchronously. */
export function getChangedFilesForHash(
  hash: Hash,
  limit = 1000,
): Promise<Result<Array<ChangedFile>>> {
  const foundPromise = allCommitFilesCache.get(hash);
  if (foundPromise != null) {
    return foundPromise;
  }
  serverAPI.postMessage({
    type: 'fetchAllCommitChangedFiles',
    hash,
    limit,
  });

  const resultPromise = serverAPI
    .nextMessageMatching('fetchedAllCommitChangedFiles', message => message.hash === hash)
    .then(result => result.result);
  allCommitFilesCache.set(hash, resultPromise);

  return resultPromise;
}
