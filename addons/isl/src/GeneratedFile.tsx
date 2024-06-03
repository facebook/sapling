/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoRelativePath} from './types';

import serverAPI from './ClientToServerAPI';
import {t} from './i18n';
import {writeAtom} from './jotaiUtils';
import {GeneratedStatus} from './types';
import {registerDisposable} from './utils';
import {atom, useAtomValue} from 'jotai';
import {useMemo} from 'react';
import {LRU} from 'shared/LRU';

export const genereatedFileCache = new LRU<RepoRelativePath, GeneratedStatus>(1500);

/** To avoid sending multiple redundant fetch requests, also save which files are being fetched right now */
const currentlyFetching = new Set<RepoRelativePath>();

/**
 * Generated files are cached in `generatedFileCache` LRU.
 * For historical reasons, the files are not an atom.
 * In order to allow rerender dependencies when we update file statuses,
 * store a generation index in recoil.
 * This state should generally be used through useGeneratedFileStatus helpers.
 */
const generatedFileGeneration = atom<number>(0);

registerDisposable(
  currentlyFetching,
  serverAPI.onMessageOfType('fetchedGeneratedStatuses', event => {
    for (const [path, status] of Object.entries(event.results)) {
      genereatedFileCache.set(path, status);
      currentlyFetching.delete(path);
    }
    writeAtom(generatedFileGeneration, old => old + 1);
  }),
  import.meta.hot,
);

export function useGeneratedFileStatus(path: RepoRelativePath): GeneratedStatus {
  useAtomValue(generatedFileGeneration); // update if we get new statuses
  const found = genereatedFileCache.get(path);
  if (found == null) {
    fetchMissingGeneratedFileStatuses([path]);
    return GeneratedStatus.Manual;
  }
  return found;
}

export function getGeneratedFilesFrom(paths: Array<RepoRelativePath>) {
  return Object.fromEntries(
    paths.map(path => [path, genereatedFileCache.get(path) ?? GeneratedStatus.Manual]),
  );
}

export function useGeneratedFileStatuses(
  paths: Array<RepoRelativePath>,
): Record<RepoRelativePath, GeneratedStatus> {
  const generation = useAtomValue(generatedFileGeneration); // update if we get new statuses

  fetchMissingGeneratedFileStatuses(paths);

  return useMemo(() => {
    return getGeneratedFilesFrom(paths);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [generation, paths]);
}

export function getCachedGeneratedFileStatuses(
  paths: Array<RepoRelativePath>,
): Record<RepoRelativePath, GeneratedStatus | undefined> {
  return Object.fromEntries(paths.map(path => [path, genereatedFileCache.get(path)]));
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

export function generatedStatusToLabel(status: GeneratedStatus | undefined): string {
  if (status === GeneratedStatus.Generated) {
    return 'generated';
  } else if (status === GeneratedStatus.PartiallyGenerated) {
    return 'partial';
  } else {
    return 'manual';
  }
}

export function generatedStatusDescription(
  status: GeneratedStatus | undefined,
): string | undefined {
  if (status === GeneratedStatus.Generated) {
    return t('This file is generated');
  } else if (status === GeneratedStatus.PartiallyGenerated) {
    return t('This file is partially generated');
  } else {
    return undefined;
  }
}
