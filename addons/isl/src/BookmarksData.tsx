/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StableLocationData} from './types';

import {atom} from 'jotai';
import {tracker} from './analytics';
import serverAPI from './ClientToServerAPI';
import {Internal} from './Internal';
import {lazyAtom, localStorageBackedAtom, readAtom, writeAtom} from './jotaiUtils';
import {latestCommits} from './serverAPIState';
import {registerDisposable} from './utils';

export const REMOTE_MASTER_BOOKMARK = 'remote/master';

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
  useRecommendedBookmark: true,
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

function fetchRecommendedBookmarks(recommendedBookmarks: Array<string>) {
  if (recommendedBookmarks) {
    serverAPI.postMessage({
      type: 'fetchRecommendedBookmarks',
      recommendedBookmarks,
    });
  }
}

/**
 * For determining if reminders to use recommended bookmarks should be shown
 */
export const recommendedBookmarksReminder = localStorageBackedAtom<{
  shouldShow: boolean;
  lastShown: number;
}>('isl.recommended-bookmarks-reminder', {
  shouldShow: true,
  lastShown: 0,
});

/**
 * For determining if recommended bookmarks onboarding tip should be shown
 */
export const recommendedBookmarksOnboarding = localStorageBackedAtom<boolean>(
  'isl.recommended-bookmarks-onboarding',
  true,
);

export const recommendedBookmarksAtom = lazyAtom(async _get => {
  const recommendedBookmarks = await (Internal.getRecommendedBookmarks?.() ??
    Promise.resolve(new Set<string>()));

  // Fetch the recommended bookmarks from server on startup
  if (recommendedBookmarks.size > 0) {
    fetchRecommendedBookmarks(Array.from(recommendedBookmarks));

    // Track recommended bookmarks to approximate adoption
    const bookmarksData = readAtom(bookmarksDataStorage);
    tracker.track('RecommendedBookmarksStatus', {
      extras: {
        enabled: bookmarksData.useRecommendedBookmark ?? false,
        recommendedBookmarks: Array.from(recommendedBookmarks),
      },
    });
  }

  return recommendedBookmarks;
}, new Set<string>());

/** Checks if recommended bookmarks are available in remoteBookmarks */
export const recommendedBookmarksAvailableAtom = atom(get => {
  const recommendedBookmarks = get(recommendedBookmarksAtom);
  const allRemoteBookmarks = get(remoteBookmarks);
  return (
    recommendedBookmarks.size > 0 &&
    [...recommendedBookmarks].some(b => allRemoteBookmarks.includes(b))
  );
});
