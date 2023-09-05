/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {__TEST__} from '../Tooltip';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  closeCommitInfoSidebar,
  simulateRepoConnected,
} from '../testUtils';
import {SucceedableRevset} from '../types';
import {fireEvent, render, screen, within} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

describe('Suggested Rebase button', () => {
  beforeEach(() => {
    resetTestMessages();
    __TEST__.resetMemoizedTooltipContainer();
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
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
    });
  });

  it('shows suggested rebase button', () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('main', 'main', '2', {phase: 'public', remoteBookmarks: ['remote/main']}),
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
    });

    expect(screen.getByText(`Rebase onto…`)).toBeInTheDocument();
  });

  it('does not show suggested rebase button on commits already on a remote bookmark', () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('main', 'main', '2', {phase: 'public', remoteBookmarks: ['remote/main']}),
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '2'), // on remote/main
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
    });

    expect(screen.queryByText('Rebase onto…')).not.toBeInTheDocument();
  });

  it('does not show suggested rebase button on commits already on a stable location', () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('main', 'main', '2', {
            phase: 'public',
            stableCommitMetadata: [{value: 'pulled here', description: ''}],
          }),
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '2'), // on stable
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
    });

    expect(screen.queryByText('Rebase onto…')).not.toBeInTheDocument();
  });

  it('shows remote bookmarks as destinations in dropdown', () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('main', 'main', '2', {
            phase: 'public',
            remoteBookmarks: ['remote/main'],
          }),
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
    });

    const rebaseOntoButton = screen.getByText('Rebase onto…');
    fireEvent.click(rebaseOntoButton);

    expect(
      within(screen.getByTestId('context-menu-container')).getByText('remote/main'),
    ).toBeInTheDocument();
  });

  it('shows stable locations as destinations in dropdown', () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('main', 'main', '2', {
            phase: 'public',
            stableCommitMetadata: [{value: 'pulled here', description: ''}],
          }),
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
    });

    const rebaseOntoButton = screen.getByText(`Rebase onto…`);
    fireEvent.click(rebaseOntoButton);

    expect(
      within(screen.getByTestId('context-menu-container')).getByText('pulled here'),
    ).toBeInTheDocument();
  });

  it('clicking suggestion rebase runs operation', () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('main', 'main', '2', {
            phase: 'public',
            remoteBookmarks: ['remote/main'],
          }),
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
    });

    const rebaseOntoButton = screen.getByText('Rebase onto…');
    fireEvent.click(rebaseOntoButton);

    const suggestion = within(screen.getByTestId('context-menu-container')).getByText(
      'remote/main',
    );
    fireEvent.click(suggestion);

    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: ['rebase', '-s', SucceedableRevset('a'), '-d', SucceedableRevset('remote/main')],
      }),
    });
  });

  it('uses hash to run rebase operation if not a remote bookmark', () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('3', 'main', '2', {
            phase: 'public',
            stableCommitMetadata: [{value: 'pulled here', description: ''}],
          }),
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
    });

    const rebaseOntoButton = screen.getByText('Rebase onto…');
    fireEvent.click(rebaseOntoButton);

    const suggestion = within(screen.getByTestId('context-menu-container')).getByText(
      'pulled here',
    );
    fireEvent.click(suggestion);

    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: ['rebase', '-s', SucceedableRevset('a'), '-d', SucceedableRevset('3')],
      }),
    });
  });
});
