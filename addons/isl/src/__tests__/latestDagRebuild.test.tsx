/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, render} from '@testing-library/react';
import App from '../App';
import {bookmarksDataStorage} from '../BookmarksData';
import {readAtom, writeAtom} from '../jotaiUtils';
import {latestDag} from '../serverAPIState';
import {COMMIT, resetTestMessages, simulateCommits, simulateRepoConnected} from '../testUtils';

describe('latestDag rebuilds', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      simulateRepoConnected();
      simulateCommits({
        value: [
          COMMIT('1', 'public base', '0', {phase: 'public', remoteBookmarks: ['remote/master']}),
          COMMIT('a', 'My Commit', '1', {grandparents: ['0']}),
          COMMIT('b', 'Another Commit', 'a', {isDot: true}),
        ],
      });
    });
  });

  // Rows skip rendering only for commits equal to their previous version, so a rebuild that
  // changes nothing about a commit must leave it equal.
  it('keeps unchanged commits equal when an input of the dag changes', () => {
    const before = readAtom(latestDag);

    act(() => {
      writeAtom(bookmarksDataStorage, data => ({
        ...data,
        hiddenRemoteBookmarks: ['remote/unrelated'],
      }));
    });
    const after = readAtom(latestDag);

    expect(after).not.toBe(before);
    for (const hash of ['1', 'a', 'b']) {
      expect(after.get(hash)?.equals(before.get(hash))).toBe(true);
    }
  });

  it('still rebuilds a public commit whose bookmarks are filtered', () => {
    act(() => {
      writeAtom(bookmarksDataStorage, data => ({
        ...data,
        hiddenRemoteBookmarks: ['remote/master'],
      }));
    });

    expect(readAtom(latestDag).get('1')?.remoteBookmarks).toEqual([]);
  });
});
