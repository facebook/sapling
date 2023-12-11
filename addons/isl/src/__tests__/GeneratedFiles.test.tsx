/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {genereatedFileCache} from '../GeneratedFile';
import {ignoreRTL} from '../testQueries';
import {
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateUncommittedChangedFiles,
  closeCommitInfoSidebar,
  simulateRepoConnected,
  resetTestMessages,
  simulateMessageFromServer,
} from '../testUtils';
import {GeneratedStatus} from '../types';
import {fireEvent, render, screen, waitFor} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

describe('Generated Files', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    genereatedFileCache.clear();
    act(() => {
      simulateRepoConnected();
      closeCommitInfoSidebar();
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: [
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'uncommittedChanges',
        subscriptionID: expect.anything(),
      });
    });
  });

  async function simulateGeneratedFiles(num: number) {
    const files = new Array(num).fill(null).map((_, i) => `file_${zeroPad(i)}.txt`);
    act(() => {
      simulateUncommittedChangedFiles({
        value: files.map(path => ({
          path,
          status: 'M',
        })),
      });
    });
    await waitFor(() => {
      expectMessageSentToServer({
        type: 'fetchGeneratedStatuses',
        paths: expect.anything(),
      });
    });
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedGeneratedStatuses',
        results: Object.fromEntries(
          files.map((path, i) => [
            path,
            i % 3 === 0
              ? GeneratedStatus.Generated
              : i % 3 === 1
              ? GeneratedStatus.PartiallyGenerated
              : GeneratedStatus.Manual,
          ]),
        ),
      });
    });
  }

  function zeroPad(n: number): string {
    return ('000' + n.toString()).slice(-3);
  }

  it('fetches generated files for uncommitted changes', async () => {
    await simulateGeneratedFiles(5);
    expectMessageSentToServer({
      type: 'fetchGeneratedStatuses',
      paths: ['file_000.txt', 'file_001.txt', 'file_002.txt', 'file_003.txt', 'file_004.txt'],
    });
  });

  it('Shows generated files in their own sections', async () => {
    await simulateGeneratedFiles(10);

    expect(screen.getByText(ignoreRTL('file_002.txt'))).toBeInTheDocument();
    expect(screen.getByText(ignoreRTL('file_005.txt'))).toBeInTheDocument();
    expect(screen.getByText(ignoreRTL('file_008.txt'))).toBeInTheDocument();
    expect(screen.getByText('Generated Files')).toBeInTheDocument();
    expect(screen.getByText('Partially Generated Files')).toBeInTheDocument();
  });

  function goToNextPage() {
    fireEvent.click(screen.getByTestId('changed-files-next-page'));
  }

  function expectHasPartiallyGeneratedFiles() {
    expect(screen.queryByText('Partially Generated Files')).toBeInTheDocument();
  }
  function expectHasGeneratedFiles() {
    expect(screen.queryByText('Generated Files')).toBeInTheDocument();
  }
  function expectNOTHasGeneratedFiles() {
    expect(screen.queryByText('Generated Files')).not.toBeInTheDocument();
  }

  function getChangedFiles() {
    const found = [...document.querySelectorAll('.changed-file-path-text')].map(e =>
      (e as HTMLElement).innerHTML.replace(/\u200E/g, ''),
    );
    return found;
  }

  it('Paginates generated files', async () => {
    await simulateGeneratedFiles(1200);
    // 1200 files, but 1000 files per fetched batch of generated statuses.
    // Sorted by status, that puts 1000/3 manual files, then 1000/3 partially generated, then 1000/3 generated,
    // then the remaining 200/3 manual, 200/3 partially generated, and 200/3 generated,
    // all in pages of 500 at a time.

    // first page is manual and partial
    expectHasPartiallyGeneratedFiles();
    expectNOTHasGeneratedFiles();
    expect(getChangedFiles()).toMatchSnapshot();

    // next page has partial and generated
    goToNextPage();
    expectHasPartiallyGeneratedFiles();
    expectHasGeneratedFiles();
    expect(getChangedFiles()).toMatchSnapshot();

    // next page has remaining files from all 3 types
    goToNextPage();
    expectHasPartiallyGeneratedFiles();
    expectHasGeneratedFiles();
    expect(getChangedFiles()).toMatchSnapshot();
  });

  it('Warns about too many files to fetch all generated statuses', async () => {
    await simulateGeneratedFiles(1001);
    expect(
      screen.getByText('There are more than 1000 files, some files may appear out of order'),
    ).toBeInTheDocument();
  });
});
