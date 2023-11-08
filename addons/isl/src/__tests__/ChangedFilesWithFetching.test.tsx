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
  simulateUncommittedChangedFiles,
  closeCommitInfoSidebar,
  simulateRepoConnected,
  resetTestMessages,
  simulateMessageFromServer,
  expectMessageNOTSentToServer,
} from '../testUtils';
import {fireEvent, render, screen, waitFor} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

function makeFiles(n: number): Array<ChangedFile> {
  return new Array(n)
    .fill(null)
    .map((_, i) => ({path: `file${i}.txt`, status: 'M'} as ChangedFile));
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
            totalFileCount: 30,
            filesSample: makeFiles(25),
          }),
          COMMIT('c', 'Another Commit 2', 'a', {
            totalFileCount: 30,
            filesSample: makeFiles(25),
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
        result: {value: makeFiles(30)},
      });
    });

    await waitForNextPageToLoad();

    expect(screen.getByText(ignoreRTL('file0.txt'))).toBeInTheDocument();
    expect(screen.getByText(ignoreRTL('file24.txt'))).toBeInTheDocument();
    expect(screen.queryByText(ignoreRTL('file25.txt'))).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('changed-files-next-page'));
    expect(screen.getByText(ignoreRTL('file25.txt'))).toBeInTheDocument();
    expect(screen.getByText(ignoreRTL('file29.txt'))).toBeInTheDocument();
  });

  it('Caches files', () => {
    CommitInfoTestUtils.clickToSelectCommit('c');
    CommitInfoTestUtils.clickToSelectCommit('a');
    resetTestMessages();

    CommitInfoTestUtils.clickToSelectCommit('c');
    expectMessageNOTSentToServer({type: 'fetchAllCommitChangedFiles', hash: expect.anything()});
  });
});
