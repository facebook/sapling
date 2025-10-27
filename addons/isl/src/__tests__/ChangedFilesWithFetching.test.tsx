/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFile, ChangedFileStatus, RepoRelativePath} from '../types';

import {act, fireEvent, render, screen, waitFor} from '@testing-library/react';
import {nextTick} from 'shared/testUtils';
import App from '../App';
import {__TEST__} from '../ChangedFilesWithFetching';
import {CommitInfoTestUtils, ignoreRTL} from '../testQueries';
import {
  COMMIT,
  expectMessageNOTSentToServer,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
  simulateRepoConnected,
} from '../testUtils';
import {leftPad} from '../utils';

function makeFiles(n: number): Array<RepoRelativePath> {
  return new Array(n).fill(null).map((_, i) => `file${leftPad(i, 3, '0')}.txt`);
}
function withStatus(files: Array<RepoRelativePath>, status: ChangedFileStatus): Array<ChangedFile> {
  return files.map(path => ({path, status}));
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
          COMMIT('1', 'some public base', '0', {phase: 'public', isDot: true}),
          COMMIT('a', 'My Commit', '1', {
            totalFileCount: 2,
            filePathsSample: ['file1.js', 'file2.js'],
          }),
          COMMIT('b', 'Another Commit', 'a', {
            totalFileCount: 700,
            filePathsSample: makeFiles(500),
          }),
          COMMIT('c', 'Another Commit 2', 'a', {
            totalFileCount: 700,
            filePathsSample: makeFiles(500),
          }),
        ],
      });
    });
  });

  afterEach(() => {
    // reset cache between tests
    __TEST__.commitFilesCache.clear();
  });

  async function waitForNextPageToLoad() {
    await waitFor(() => {
      expect(screen.getByTestId('changed-files-next-page')).toBeInTheDocument();
    });
  }

  it("Does not fetch files if they're already all fetched", () => {
    CommitInfoTestUtils.clickToSelectCommit('a');

    expectMessageNOTSentToServer({
      type: 'fetchCommitChangedFiles',
      hash: 'a',
      limit: expect.anything(),
    });
  });

  it('Fetches files and shows additional pages', async () => {
    CommitInfoTestUtils.clickToSelectCommit('b');

    expectMessageSentToServer({type: 'fetchCommitChangedFiles', hash: 'b', limit: undefined});
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedCommitChangedFiles',
        hash: 'b',
        result: {value: {filesSample: withStatus(makeFiles(510), 'M'), totalFileCount: 510}},
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
    expectMessageNOTSentToServer({type: 'fetchCommitChangedFiles', hash: expect.anything()});
  });

  it('Handles race condition when switching commits and responses come in wrong order', async () => {
    // Select commit 'b' - this will start fetching files
    CommitInfoTestUtils.clickToSelectCommit('b');
    expectMessageSentToServer({type: 'fetchCommitChangedFiles', hash: 'b', limit: undefined});

    // Before 'b' response comes back, switch to commit 'c'
    CommitInfoTestUtils.clickToSelectCommit('c');
    expectMessageSentToServer({type: 'fetchCommitChangedFiles', hash: 'c', limit: undefined});

    // Simulate 'c' response coming back first (fast)
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedCommitChangedFiles',
        hash: 'c',
        result: {
          value: {
            filesSample: withStatus(['fileC1.txt', 'fileC2.txt', 'fileC3.txt'], 'M'),
            totalFileCount: 3,
          },
        },
      });
    });

    // Verify we're showing commit 'c' files
    await waitFor(() => {
      expect(screen.getByText(ignoreRTL('fileC1.txt'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('fileC2.txt'))).toBeInTheDocument();
    });

    // Now simulate 'b' response coming back late (slow)
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedCommitChangedFiles',
        hash: 'b',
        result: {
          value: {
            filesSample: withStatus(['fileB1.txt', 'fileB2.txt', 'fileB3.txt'], 'M'),
            totalFileCount: 3,
          },
        },
      });
    });

    // Wait a bit to ensure any state updates have completed
    await nextTick();

    await waitFor(() => {
      // The files should STILL be from commit 'c', not 'b'
      // With the fix enabled, the cancellation token prevents 'b' from overwriting 'c'
      expect(screen.getByText(ignoreRTL('fileC1.txt'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('fileC2.txt'))).toBeInTheDocument();
    });

    // Verify we're NOT showing commit 'b' files
    expect(screen.queryByText(ignoreRTL('fileB1.txt'))).not.toBeInTheDocument();
    expect(screen.queryByText(ignoreRTL('fileB2.txt'))).not.toBeInTheDocument();
  });
});
