/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {localStorageBackedAtom} from './jotaiUtils';
import {dagWithPreviews} from './previews';
import {atom} from 'jotai';

type BookmarksData = {
  /** These bookmarks should be hidden from the automatic set of remote bookmarks */
  hiddenRemoteBookmarks: Array<string>;
};
export const bookmarksDataStorage = localStorageBackedAtom<BookmarksData>('isl.bookmarks', {
  hiddenRemoteBookmarks: [],
});

export const remoteBookmarks = atom(get => {
  const dag = get(dagWithPreviews);
  return dag.getBatch(dag.public_().toArray()).flatMap(commit => commit.remoteBookmarks);
});
