/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {localStorageBackedAtom} from './jotaiUtils';

type BookmarksData = {
  /** These bookmarks should be hidden from the automatic set of remote bookmarks */
  hiddenRemoteBookmarks: Array<string>;
};
export const bookmarksDataStorage = localStorageBackedAtom<BookmarksData>('isl.bookmarks', {
  hiddenRemoteBookmarks: [],
});
