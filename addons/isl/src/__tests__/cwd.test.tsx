/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {CommitTreeListTestUtils} from '../testQueries';
import {
  closeCommitInfoSidebar,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateMessageFromServer,
  resetTestMessages,
  simulateRepoConnected,
} from '../testUtils';
import {fireEvent, render, screen, within} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

const {clickGoto} = CommitTreeListTestUtils;

describe('cwd', () => {
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
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
    });
  });

  const openCwdDropdown = () => {
    const cwdDropdown = screen.getByTestId('cwd-dropdown-button');
    act(() => {
      fireEvent.click(cwdDropdown);
    });
    expectMessageSentToServer({
      type: 'platform/subscribeToAvailableCwds',
    });
    act(() => {
      simulateMessageFromServer({
        type: 'platform/availableCwds',
        options: ['/path/to/repo1', '/path/to/repo2'],
      });
    });
  };

  it('shows cwd options from the platform', () => {
    openCwdDropdown();

    const dropdown = screen.getByTestId('cwd-details-dropdown');

    expect(within(dropdown).getByText('repo1')).toBeInTheDocument();
    expect(within(dropdown).getByText('repo2')).toBeInTheDocument();
  });

  it('requests new data for subscriptions after changing cwd', () => {
    openCwdDropdown();

    resetTestMessages();

    const dropdown = screen.getByTestId('cwd-details-dropdown');
    act(() => {
      fireEvent.click(within(dropdown).getByText('repo2'));
    });

    expectMessageSentToServer({type: 'changeCwd', cwd: '/path/to/repo2'});
    expectMessageSentToServer({type: 'requestRepoInfo'});
    expectMessageSentToServer({
      type: 'subscribe',
      kind: 'smartlogCommits',
      subscriptionID: expect.anything(),
    });
    expectMessageSentToServer({
      type: 'subscribe',
      kind: 'uncommittedChanges',
      subscriptionID: expect.anything(),
    });
  });

  it('clears out saved state when changing repos', () => {
    clickGoto('a');

    expect(screen.getByText('sl goto --rev a')).toBeInTheDocument();

    openCwdDropdown();

    resetTestMessages();

    const dropdown = screen.getByTestId('cwd-details-dropdown');
    act(() => {
      fireEvent.click(within(dropdown).getByText('repo2'));
    });

    expect(screen.queryByText('sl goto --rev a')).not.toBeInTheDocument();
  });

  it('dismisses dropdown when changing cwd', () => {
    openCwdDropdown();

    const dropdown = screen.getByTestId('cwd-details-dropdown');
    act(() => {
      fireEvent.click(within(dropdown).getByText('repo2'));
    });

    expect(screen.queryByTestId('cwd-details-dropdown')).not.toBeInTheDocument();
  });
});
