/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StableLocationData} from './types';

import {atom} from 'jotai';
import serverAPI from './ClientToServerAPI';
import {localStorageBackedAtom, readAtom, writeAtom} from './jotaiUtils';
import {latestCommits} from './serverAPIState';
import {registerDisposable} from './utils';

export type BookmarksData = {
  /** These bookmarks should be hidden from the automatic set of remote bookmarks */
  hiddenRemoteBookmarks: Array<string>;
  /** These stables should be requested by the server to fetch additional stables */
  additionalStables?: Array<string>;
  /** Whether to use the recommended bookmark instead of user-selected bookmarks */
  useRecommendedBookmark?: boolean;
};
export const bookmarksDataStorage = localStorageBackedAtom<BookmarksData>('isl.bookmarks', {
  hiddenRemoteBookmarks: [],
  additionalStables: [],
  useRecommendedBookmark: false,
});
export const hiddenRemoteBookmarksAtom = atom(get => {
  return new Set(get(bookmarksDataStorage).hiddenRemoteBookmarks);
});

/** Result of fetch from the server. Stables are automatically included in list of commits */
export const fetchedStablesAtom = atom<StableLocationData | undefined>(undefined);

export function addManualStable(stable: string) {
  // save this as a persisted stable we'd like to always fetch going forward
  writeAtom(bookmarksDataStorage, data => ({
    ...data,
    additionalStables: [...(data.additionalStables ?? []), stable],
  }));
  // write the stable to the fetched state, so it shows a loading spinner
  writeAtom(fetchedStablesAtom, last =>
    last
      ? {
          ...last,
          manual: {...(last?.manual ?? {}), [stable]: null},
        }
      : undefined,
  );
  // refetch using the new manual stable
  fetchStableLocations();
}

export function removeManualStable(stable: string) {
  writeAtom(bookmarksDataStorage, data => ({
    ...data,
    additionalStables: (data.additionalStables ?? []).filter(s => s !== stable),
  }));
  writeAtom(fetchedStablesAtom, last => {
    if (last) {
      const manual = {...(last.manual ?? {})};
      delete manual[stable];
      return {...last, manual};
    }
  });
  // refetch without this stable, so it's excluded from `sl log`
  fetchStableLocations();
}

registerDisposable(
  serverAPI,
  serverAPI.onMessageOfType('fetchedStables', data => {
    writeAtom(fetchedStablesAtom, data.stables);
  }),
  import.meta.hot,
);
fetchStableLocations(); // fetch on startup

export function fetchStableLocations() {
  const data = readAtom(bookmarksDataStorage);
  const additionalStables = data.additionalStables ?? [];
  serverAPI.postMessage({type: 'fetchAndSetStables', additionalStables});
}

export const remoteBookmarks = atom(get => {
  // Note: `latestDag` will have already filtered out hidden bookmarks,
  // so we need to use latestCommits, which is not filtered.
  const commits = get(latestCommits).filter(commit => commit.phase === 'public');
  commits.sort((a, b) => b.date.valueOf() - a.date.valueOf());
  return commits.flatMap(commit => commit.remoteBookmarks);
});
