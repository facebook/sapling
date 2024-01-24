/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFile} from '../types';

import App from '../App';
import {ignoreRTL, CommitInfoTestUtils} from '../testQueries';
import {
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateRepoConnected,
  resetTestMessages,
  simulateMessageFromServer,
  expectMessageNOTSentToServer,
} from '../testUtils';
import {leftPad} from '../utils';
import {fireEvent, render, screen, waitFor} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

function makeFiles(n: number): Array<ChangedFile> {
  return new Array(n)
    .fill(null)
    .map((_, i) => ({path: `file${leftPad(i, 3, '0')}.txt`, status: 'M'} as ChangedFile));
}

describe('ChangedFilesWithFetching', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      simulateRepoConnected();
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: [
          COMMIT('1', 'some public base', '0', {phase: 'public', isHead: true}),
          COMMIT('a', 'My Commit', '1', {
            totalFileCount: 2,
            filesSample: [
              {path: 'file1.js', status: 'M'},
              {path: 'file2.js', status: 'M'},
            ],
          }),
          COMMIT('b', 'Another Commit', 'a', {
            totalFileCount: 700,
            filesSample: makeFiles(500),
          }),
          COMMIT('c', 'Another Commit 2', 'a', {
            totalFileCount: 700,
            filesSample: makeFiles(500),
          }),
        ],
      });
    });
  });

  async function waitForNextPageToLoad() {
    await waitFor(() => {
      expect(screen.getByTestId('changed-files-next-page')).toBeInTheDocument();
    });
  }

  it("Does not fetch files if they're already all fetched", () => {
    CommitInfoTestUtils.clickToSelectCommit('a');

    expectMessageNOTSentToServer({type: 'fetchAllCommitChangedFiles', hash: expect.anything()});
  });

  it('Fetches files and shows additional pages', async () => {
    CommitInfoTestUtils.clickToSelectCommit('b');

    expectMessageSentToServer({type: 'fetchAllCommitChangedFiles', hash: 'b'});
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedAllCommitChangedFiles',
        hash: 'b',
        result: {value: makeFiles(510)},
      });
    });

    await waitForNextPageToLoad();

    expect(screen.getByText(ignoreRTL('file000.txt'))).toBeInTheDocument();
    expect(screen.getByText(ignoreRTL('file499.txt'))).toBeInTheDocument();
    expect(screen.queryByText(ignoreRTL('file500.txt'))).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('changed-files-next-page'));
    expect(screen.queryByText(ignoreRTL('file499.txt'))).not.toBeInTheDocument();
    expect(screen.getByText(ignoreRTL('file500.txt'))).toBeInTheDocument();
    expect(screen.getByText(ignoreRTL('file509.txt'))).toBeInTheDocument();
  });

  it('Caches files', () => {
    CommitInfoTestUtils.clickToSelectCommit('c');
    CommitInfoTestUtils.clickToSelectCommit('a');
    resetTestMessages();

    CommitInfoTestUtils.clickToSelectCommit('c');
    expectMessageNOTSentToServer({type: 'fetchAllCommitChangedFiles', hash: expect.anything()});
  });
});
