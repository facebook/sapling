/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen, waitFor} from '@testing-library/react';
import App from '../App';
import platform from '../platform';
import {ignoreRTL} from '../testQueries';
import {
  closeCommitInfoSidebar,
  COMMIT,
  expectMessageSentToServer,
  openCommitInfoSidebar,
  simulateCommits,
  simulateMessageFromServer,
  simulateRepoConnected,
  simulateUncommittedChangedFiles,
} from '../testUtils';

describe('Browse repo url', () => {
  beforeEach(() => {
    render(<App />);
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
          COMMIT('1', 'some public base', '0', {phase: 'public', remoteBookmarks: ['main']}),
          COMMIT('a', 'My Commit', '1', {isDot: true}),
        ],
      });
    });
  });

  function setCodeBrowserConfig(value: string | undefined) {
    expectMessageSentToServer({type: 'getConfig', name: 'fbcodereview.code-browser-url'});
    act(() => {
      simulateMessageFromServer({
        type: 'gotConfig',
        name: 'fbcodereview.code-browser-url',
        value,
      });
    });
  }

  function clickBrowseRepo() {
    act(() => {
      fireEvent.contextMenu(screen.getByText('main'));
    });
    expect(screen.getByText('Browse Repo At This Commit')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Browse Repo At This Commit'));
  }

  it('opens link to browse repo', () => {
    setCodeBrowserConfig(undefined);
    act(() => {
      fireEvent.contextMenu(screen.getByText('main'));
    });
    expect(screen.queryByText('Browse Repo At This Commit')).not.toBeInTheDocument();
  });

  it('opens link to browse repo', async () => {
    setCodeBrowserConfig('https://www.example.com/repo/browse/%s');
    clickBrowseRepo();

    const openLinkSpy = jest.spyOn(platform, 'openExternalLink').mockImplementation(() => {});
    expectMessageSentToServer({
      type: 'getRepoUrlAtHash',
      revset: '1',
      path: undefined,
    });
    act(() => {
      simulateMessageFromServer({
        type: 'gotRepoUrlAtHash',
        url: {value: 'https://www.example.com/repo/browse/1/'},
      });
    });

    await waitFor(() =>
      expect(openLinkSpy).toHaveBeenCalledWith('https://www.example.com/repo/browse/1/'),
    );
  });

  it('surfaces errors', async () => {
    setCodeBrowserConfig('https://www.example.com/repo/browse/%s');
    clickBrowseRepo();

    const openLinkSpy = jest.spyOn(platform, 'openExternalLink').mockImplementation(() => {});
    expectMessageSentToServer({
      type: 'getRepoUrlAtHash',
      revset: '1',
      path: undefined,
    });
    act(() => {
      simulateMessageFromServer({
        type: 'gotRepoUrlAtHash',
        url: {error: new Error('failed')},
      });
    });

    await waitFor(() => {
      expect(openLinkSpy).not.toHaveBeenCalled();
      expect(screen.getByText('Failed to get repo URL to browse')).toBeInTheDocument();
    });
  });

  describe('Copy file URL', () => {
    it('copies link to file repo', async () => {
      act(() => {
        simulateUncommittedChangedFiles({value: [{path: 'file1.txt', status: 'M'}]});
      });
      setCodeBrowserConfig('https://www.example.com/repo/browse/%s');
      act(() => {
        fireEvent.contextMenu(screen.getByText(ignoreRTL('file1.txt')));
      });
      expect(screen.getByText('Copy file URL')).toBeInTheDocument();
      fireEvent.click(screen.getByText('Copy file URL'));

      expectMessageSentToServer({
        type: 'getRepoUrlAtHash',
        revset: '.',
        path: 'file1.txt',
      });
      act(() => {
        simulateMessageFromServer({
          type: 'gotRepoUrlAtHash',
          url: {value: 'https://www.example.com/repo/browse/a/file1.txt'},
        });
      });

      await waitFor(() => {
        const copySpy = jest.spyOn(platform, 'clipboardCopy').mockImplementation(() => {});
        expect(copySpy).toHaveBeenCalledWith(
          'https://www.example.com/repo/browse/a/file1.txt',
          undefined,
        );
        expect(
          screen.getByText('Copied https://www.example.com/repo/browse/a/file1.txt'),
        ).toBeInTheDocument();
      });
    });

    it('uses appropricate commit revset', () => {
      act(() => {
        openCommitInfoSidebar();
      });
      setCodeBrowserConfig('https://www.example.com/repo/browse/%s');
      act(() => {
        simulateCommits({
          value: [
            COMMIT('1', 'some public base', '0', {phase: 'public', remoteBookmarks: ['main']}),
            COMMIT('a', 'My Commit', '1', {
              isDot: true,
              totalFileCount: 1,
              filePathsSample: ['file2.txt'],
            }),
          ],
        });
      });

      act(() => {
        fireEvent.contextMenu(screen.getByText(ignoreRTL('file2.txt')));
      });
      expect(screen.getByText('Copy file URL')).toBeInTheDocument();
      fireEvent.click(screen.getByText('Copy file URL'));

      expectMessageSentToServer({
        type: 'getRepoUrlAtHash',
        revset: '.^',
        path: 'file2.txt',
      });
    });
  });
});
