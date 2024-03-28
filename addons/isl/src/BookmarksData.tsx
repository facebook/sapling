/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StableLocationData} from './types';

import serverAPI from './ClientToServerAPI';
import {localStorageBackedAtom, writeAtom} from './jotaiUtils';
import {latestCommits} from './serverAPIState';
import {registerDisposable} from './utils';
import {atom} from 'jotai';

type BookmarksData = {
  /** These bookmarks should be hidden from the automatic set of remote bookmarks */
  hiddenRemoteBookmarks: Array<string>;
};
export const bookmarksDataStorage = localStorageBackedAtom<BookmarksData>('isl.bookmarks', {
  hiddenRemoteBookmarks: [],
});
export const hiddenRemoteBookmarksAtom = atom(get => {
  return new Set(get(bookmarksDataStorage).hiddenRemoteBookmarks);
});

/** Result of fetch from the server. Stables are automatically included in list of commits */
export const fetchedStablesAtom = atom<StableLocationData | undefined>(undefined);

registerDisposable(
  serverAPI,
  serverAPI.onMessageOfType('fetchedStables', data => {
    writeAtom(fetchedStablesAtom, data.stables);
  }),
  import.meta.hot,
);
fetchStableLocations(); // fetch on startup

export function fetchStableLocations() {
  serverAPI.postMessage({type: 'fetchAndSetStables'});
}

export const remoteBookmarks = atom(get => {
  // Note: `latestDag` will have already filtered out hidden bookmarks,
  // so we need to use latestCommits, which is not filtered.
  const commits = get(latestCommits).filter(commit => commit.phase === 'public');
  commits.sort((a, b) => b.date.valueOf() - a.date.valueOf());
  return commits.flatMap(commit => commit.remoteBookmarks);
});
