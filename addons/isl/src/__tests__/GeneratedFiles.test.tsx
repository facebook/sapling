/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen, waitFor} from '@testing-library/react';
import App from '../App';
import {generatedFileCache} from '../GeneratedFile';
import {__TEST__} from '../UncommittedChanges';
import {readAtom, writeAtom} from '../jotaiUtils';
import platform from '../platform';
import {ignoreRTL} from '../testQueries';
import {
  closeCommitInfoSidebar,
  COMMIT,
  expectMessageSentToServer,
  openCommitInfoSidebar,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
  simulateRepoConnected,
  simulateUncommittedChangedFiles,
} from '../testUtils';
import {GeneratedStatus} from '../types';

/** Generated `num` files, in the repeating pattern: generated, partially generated, manual */
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

describe('Generated Files', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    generatedFileCache.clear();
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
          COMMIT('b', 'Another Commit', 'a', {isDot: true}),
        ],
      });
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'uncommittedChanges',
        subscriptionID: expect.anything(),
      });
    });
  });

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

  it('remembers expanded state', async () => {
    writeAtom(__TEST__.generatedFilesInitiallyExpanded, true);

    await simulateGeneratedFiles(1);

    expect(screen.getByText(ignoreRTL('file_000.txt'))).toBeInTheDocument();
    expect(screen.getByText('Generated Files')).toBeInTheDocument();
  });

  it('writes expanded state', async () => {
    expect(readAtom(__TEST__.generatedFilesInitiallyExpanded)).toEqual(false);

    await simulateGeneratedFiles(1);

    fireEvent.click(screen.getByText('Generated Files'));

    expect(readAtom(__TEST__.generatedFilesInitiallyExpanded)).toEqual(true);
  });

  it('clears generated files cache on refresh click', async () => {
    act(() => {
      simulateUncommittedChangedFiles({
        value: [
          {
            path: 'file.txt',
            status: 'M',
          },
        ],
      });
    });
    await waitFor(() => {
      expectMessageSentToServer({
        type: 'fetchGeneratedStatuses',
        paths: ['file.txt'],
      });
    });
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedGeneratedStatuses',
        results: Object.fromEntries([['file.txt', GeneratedStatus.Manual]]),
      });
    });

    expect(screen.queryByText('Generated Files')).not.toBeInTheDocument();

    act(() => {
      fireEvent.click(screen.getByTestId('refresh-button'));
    });
    await waitFor(() => {
      expectMessageSentToServer({
        type: 'fetchGeneratedStatuses',
        paths: ['file.txt'],
      });
    });
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedGeneratedStatuses',
        results: Object.fromEntries([['file.txt', GeneratedStatus.Generated]]),
      });
    });

    expect(screen.getByText('Generated Files')).toBeInTheDocument();
  });

  describe('Open All Files', () => {
    beforeEach(() => act(() => openCommitInfoSidebar()));
    async function simulateCommitWithFiles(files: Record<string, GeneratedStatus>) {
      act(() => {
        simulateCommits({
          value: [
            COMMIT('1', 'some public base', '0', {phase: 'public'}),
            COMMIT('a', 'Commit A', '1', {
              isDot: true,
              totalFileCount: 3,
              filePathsSample: Object.keys(files),
            }),
          ],
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
          results: files,
        });
      });
    }

    it('No generated files, opens all files', async () => {
      const openSpy = jest.spyOn(platform, 'openFiles').mockImplementation(() => {});
      await simulateCommitWithFiles({
        'file_partial.txt': GeneratedStatus.PartiallyGenerated,
        'file_manual.txt': GeneratedStatus.Manual,
      });

      fireEvent.click(screen.getByText('Open All Files'));
      expect(openSpy).toHaveBeenCalledTimes(1);
      expect(openSpy).toHaveBeenCalledWith(['file_partial.txt', 'file_manual.txt']);
    });

    it('Some generated files, opens all non-generated files', async () => {
      const openSpy = jest.spyOn(platform, 'openFiles').mockImplementation(() => {});
      await simulateCommitWithFiles({
        'file_gen.txt': GeneratedStatus.Generated,
        'file_partial.txt': GeneratedStatus.PartiallyGenerated,
        'file_manual.txt': GeneratedStatus.Manual,
      });

      fireEvent.click(screen.getByText('Open Non-Generated Files'));
      expect(openSpy).toHaveBeenCalledTimes(1);
      expect(openSpy).toHaveBeenCalledWith(['file_partial.txt', 'file_manual.txt']);
    });

    it('All generated files, opens all files', async () => {
      const openSpy = jest.spyOn(platform, 'openFiles').mockImplementation(() => {});
      await simulateCommitWithFiles({
        'file_gen1.txt': GeneratedStatus.Generated,
        'file_gen2.txt': GeneratedStatus.Generated,
      });

      fireEvent.click(screen.getByText('Open All Files'));
      expect(openSpy).toHaveBeenCalledTimes(1);
      expect(openSpy).toHaveBeenCalledWith(['file_gen1.txt', 'file_gen2.txt']);
    });
  });
});
