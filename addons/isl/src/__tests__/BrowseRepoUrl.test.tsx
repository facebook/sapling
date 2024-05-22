/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import foundPlatform from '../platform';
import {
  closeCommitInfoSidebar,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateRepoConnected,
  simulateMessageFromServer,
} from '../testUtils';
import {render, screen, fireEvent, act, waitFor} from '@testing-library/react';

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

  it('opens link to browse repo', async () => {
    setCodeBrowserConfig(undefined);
    act(() => {
      fireEvent.contextMenu(screen.getByText('main'));
    });
    expect(screen.queryByText('Browse Repo At This Commit')).not.toBeInTheDocument();
  });

  it('opens link to browse repo', async () => {
    setCodeBrowserConfig('https://www.example.com/repo/browse/%s');
    clickBrowseRepo();

    const openLinkSpy = jest.spyOn(foundPlatform, 'openExternalLink').mockImplementation(() => {});
    expectMessageSentToServer({
      type: 'getRepoUrlAtHash',
      hash: '1',
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

    const openLinkSpy = jest.spyOn(foundPlatform, 'openExternalLink').mockImplementation(() => {});
    expectMessageSentToServer({
      type: 'getRepoUrlAtHash',
      hash: '1',
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
});
