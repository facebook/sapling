/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import App from '../App';
import {recommendedBookmarksAtom} from '../BookmarksData';
import {readAtom} from '../jotaiUtils';
import platform from '../platform';
import {
  closeCommitInfoSidebar,
  COMMIT,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
  simulateRepoConnected,
  TEST_COMMIT_HISTORY,
} from '../testUtils';

/*eslint-disable @typescript-eslint/no-non-null-assertion */

describe('bookmarks', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      closeCommitInfoSidebar();
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: TEST_COMMIT_HISTORY,
      });
    });
  });

  it('lets you create bookmarks', () => {
    act(() => {
      fireEvent.contextMenu(screen.getByText('Commit A'));
    });
    act(() => {
      fireEvent.click(screen.getByText('Create Bookmark...'));
    });

    expect(screen.getByLabelText('Bookmark Name')).toBeInTheDocument();
    expect(screen.getByLabelText('Bookmark Name')).toHaveFocus();
    expect(screen.getByText('Create')).toBeInTheDocument();
    expect(screen.getByText('Create')).toBeDisabled();
    act(() => {
      userEvent.type(screen.getByLabelText('Bookmark Name'), 'testBook');
    });
    expect(screen.getByText('Create')).not.toBeDisabled();
    fireEvent.click(screen.getByText('Create'));

    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: ['bookmark', 'testBook', '--rev', {type: 'succeedable-revset', revset: 'a'}],
      }),
    });
  });

  it('lets you delete bookmarks', () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('1', 'public commit', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1'),
          COMMIT('b', 'Commit B', 'a', {isDot: true, bookmarks: ['myBookmark']}),
        ],
      });
    });
    expect(screen.getByText('myBookmark')).toBeInTheDocument();
    act(() => {
      fireEvent.contextMenu(screen.getByText('myBookmark'));
    });
    const button = screen.getByText('Delete Bookmark "myBookmark"');
    expect(button).toBeInTheDocument();
    fireEvent.click(button);

    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: ['bookmark', '--delete', 'myBookmark'],
      }),
    });
  });

  it('loads and updates cached recommended bookmarks for the current repository', () => {
    const repoRoot = '/repo/with-cached-recommendations';
    const storageKey = `isl.recommended-bookmarks:${repoRoot}`;
    platform.setPersistedState(storageKey, {
      data: ['remote/cached'],
      date: Date.now(),
    });

    act(() => simulateRepoConnected(repoRoot));
    expect(readAtom(recommendedBookmarksAtom)).toEqual(new Set(['remote/cached']));

    act(() => {
      simulateMessageFromServer({
        type: 'fetchedRecommendedBookmarks',
        repoRoot,
        bookmarks: ['remote/fetched'],
      });
    });
    expect(readAtom(recommendedBookmarksAtom)).toEqual(new Set(['remote/fetched']));
    expect(platform.getPersistedState(storageKey)).toEqual({
      data: ['remote/fetched'],
      date: expect.any(Number),
    });

    act(() => simulateRepoConnected('/different/repo'));
    expect(readAtom(recommendedBookmarksAtom)).toEqual(new Set());

    const earlyRepoRoot = '/repo/response-before-info';
    const earlyStorageKey = `isl.recommended-bookmarks:${earlyRepoRoot}`;
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedRecommendedBookmarks',
        repoRoot: earlyRepoRoot,
        bookmarks: ['remote/early-response'],
      });
    });
    expect(readAtom(recommendedBookmarksAtom)).toEqual(new Set());

    act(() => simulateRepoConnected(earlyRepoRoot));
    expect(readAtom(recommendedBookmarksAtom)).toEqual(new Set(['remote/early-response']));

    platform.setPersistedState(storageKey, undefined);
    platform.setPersistedState(earlyStorageKey, undefined);
  });
});
