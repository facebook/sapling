/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoRelativePath} from './types';

import serverAPI from './ClientToServerAPI';
import {GeneratedStatus} from './types';
import {DefaultValue, atom, useRecoilValue} from 'recoil';
import {LRU} from 'shared/LRU';

export const genereatedFileCache = new LRU<RepoRelativePath, GeneratedStatus>(1000);

/** To avoid sending multiple redundant fetch requests, also save which files are being fetched right now */
const currentlyFetching = new Set<RepoRelativePath>();

/**
 * Generated files are cached in `generatedFileCache` LRU.
 * This is not part of a recoil atom so it can be accessed anywhere.
 * In order to allow recoil to rerender dependencies when we update file statuses,
 * store a generation index in recoil.
 * This state should generally be used through useGeneratedFileStatus helpers.
 */
const generatedFileGeneration = atom<number>({
  key: 'generatedFileGeneration',
  default: 0,
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('fetchedGeneratedStatuses', event => {
        for (const [path, status] of Object.entries(event.results)) {
          genereatedFileCache.set(path, status);
          currentlyFetching.delete(path);
        }
        setSelf(old => (old instanceof DefaultValue ? 1 : old + 1));
      });
      return () => disposable.dispose();
    },
  ],
});

export function useGeneratedFileStatus(path: RepoRelativePath): GeneratedStatus {
  useRecoilValue(generatedFileGeneration); // update if we get new statuses
  const found = genereatedFileCache.get(path);
  if (found == null) {
    fetchMissingGeneratedFileStatuses([path]);
    return GeneratedStatus.Manual;
  }
  return found;
}
export function useGeneratedFileStatuses(
  paths: Array<RepoRelativePath>,
): Record<RepoRelativePath, GeneratedStatus> {
  useRecoilValue(generatedFileGeneration); // update if we get new statuses

  fetchMissingGeneratedFileStatuses(paths);

  return Object.fromEntries(
    paths.map(path => [path, genereatedFileCache.get(path) ?? GeneratedStatus.Manual]),
  );
}

/**
 * Hint that this set of files are being used, any files missing from the generated files cache
 * should be checked on the server.
 * No-op if all files already in the cache.
 */
export function fetchMissingGeneratedFileStatuses(files: Array<RepoRelativePath>) {
  const notCached = files.filter(
    file => genereatedFileCache.get(file) == null && !currentlyFetching.has(file),
  );
  if (notCached.length > 0) {
    for (const file of notCached) {
      currentlyFetching.add(file);
    }
    serverAPI.postMessage({
      type: 'fetchGeneratedStatuses',
      paths: notCached,
    });
  }
}
