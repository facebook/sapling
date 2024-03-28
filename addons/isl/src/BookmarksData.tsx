/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StableLocationData} from './types';

import serverAPI from './ClientToServerAPI';
import {localStorageBackedAtom, writeAtom} from './jotaiUtils';
import {dagWithPreviews} from './previews';
import {registerDisposable} from './utils';
import {atom} from 'jotai';

type BookmarksData = {
  /** These bookmarks should be hidden from the automatic set of remote bookmarks */
  hiddenRemoteBookmarks: Array<string>;
};
export const bookmarksDataStorage = localStorageBackedAtom<BookmarksData>('isl.bookmarks', {
  hiddenRemoteBookmarks: [],
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
  const dag = get(dagWithPreviews);
  const commits = dag.getBatch(dag.public_().toArray());
  commits.sort((a, b) => b.date.valueOf() - a.date.valueOf());
  return commits.flatMap(commit => commit.remoteBookmarks);
});
