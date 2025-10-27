/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFile, CommitInfo, FilesSample, Hash, Result} from './types';

import {Button} from 'isl-components/Button';
import {Tooltip} from 'isl-components/Tooltip';
import {useEffect, useState} from 'react';
import {CancellationToken} from 'shared/CancellationToken';
import {ComparisonType} from 'shared/Comparison';
import {LRU} from 'shared/LRU';
import serverAPI from './ClientToServerAPI';
import {ChangedFiles} from './UncommittedChanges';
import {t, T} from './i18n';

// Cache fetches in progress so we don't double fetch
const commitFilesCache = new LRU<Hash, Promise<Result<FilesSample>>>(10);
export const __TEST__ = {commitFilesCache};

/**
 * The basic CommitInfo we fetch in bulk only contains the first 25 files,
 * and is missing file statuses.
 * But we want to be able to scroll through pages of files,
 * and also see their statuses (added, removed, etc).
 * So all files for the currently selected commit,
 * to augment the subset we already have.
 * Public commits don't show changed files by default for performance,
 * but instead show a button used to fetch all the files.
 */
export function ChangedFilesWithFetching({commit}: {commit: CommitInfo}) {
  const [fetchedAllFiles, setFetchedAllFiles] = useState<FilesSample | undefined>(undefined);

  const [showingPublicWarning, setShowPublicWarning] = useState(commit.phase === 'public');
  useEffect(() => {
    setShowPublicWarning(commit.phase === 'public');
  }, [commit.hash, commit.phase]);

  useEffect(() => {
    if (showingPublicWarning === true) {
      return;
    }

    setFetchedAllFiles(undefined);
    const cancel = new CancellationToken();
    getChangedFilesForHash(commit.hash, undefined).then(result => {
      if (cancel.isCancelled) {
        return;
      }
      if (result.value != null) {
        setFetchedAllFiles(result.value);
      }
    });
    return () => {
      cancel.cancel();
    };
  }, [commit.hash, showingPublicWarning]);

  if (showingPublicWarning) {
    return (
      <Tooltip
        title={t(
          'Changed files are not loaded for public commits by default, for performance. Click to load changed files.',
        )}>
        <Button onClick={() => setShowPublicWarning(false)}>
          <T>Load changed files</T>
        </Button>
      </Tooltip>
    );
  }

  return (
    <ChangedFiles
      filesSubset={
        fetchedAllFiles?.filesSample ??
        commit.filePathsSample.map(
          (filePath): ChangedFile => ({
            path: filePath,
            // default to 'modified' as a best guess.
            // TODO: should this be a special loading status that shows a spinner?
            status: 'M' as const,
          }),
        )
      }
      totalFiles={fetchedAllFiles?.totalFileCount ?? commit.totalFileCount}
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

/**
 * Get changed files in a given commit.
 * A small subset of the files may have already been fetched,
 * or in some cases no files may be cached yet and all files need to be fetched asynchronously. */
export function getChangedFilesForHash(
  hash: Hash,
  limit?: number | undefined,
): Promise<Result<FilesSample>> {
  const foundPromise = commitFilesCache.get(hash);
  if (foundPromise != null) {
    return foundPromise;
  }
  serverAPI.postMessage({
    type: 'fetchCommitChangedFiles',
    hash,
    limit,
  });

  const resultPromise = serverAPI
    .nextMessageMatching('fetchedCommitChangedFiles', message => message.hash === hash)
    .then(result => result.result);
  commitFilesCache.set(hash, resultPromise);

  return resultPromise;
}
