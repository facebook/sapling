/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen, within} from '@testing-library/react';
import App from '../App';
import {__TEST__} from '../repositoryData';
import {CommitTreeListTestUtils} from '../testQueries';
import {
  closeCommitInfoSidebar,
  COMMIT,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
  simulateRepoConnected,
} from '../testUtils';
const {isIrrelevantToCwd} = __TEST__;

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
          COMMIT('b', 'Another Commit', 'a', {isDot: true}),
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
        options: [
          {
            cwd: '/path/to/repo1',
            repoRoot: '/path/to/repo1',
            repoRelativeCwdLabel: 'repo1',
          },
          {
            cwd: '/path/to/repo2',
            repoRoot: '/path/to/repo2',
            repoRelativeCwdLabel: 'repo2',
          },
          {
            cwd: '/path/to/repo2/some/subdir',
            repoRoot: '/path/to/repo2',
            repoRelativeCwdLabel: 'repo2/some/subdir',
          },
        ],
      });
    });
  };

  it('shows repo+relative cwd in the cwd button', () => {
    act(() => {
      simulateRepoConnected('/path/to/repo', '/path/to/repo/some/subdir');
    });
    expect(screen.getByText('repo/some/subdir')).toBeInTheDocument();

    act(() => {
      simulateRepoConnected('C:\\path\\to\\repo', 'C:\\path\\to\\repo\\some\\subdir');
    });
    expect(screen.getByText('repo\\some\\subdir')).toBeInTheDocument();
  });

  it('shows cwd options from the platform', () => {
    openCwdDropdown();

    const dropdown = screen.getByTestId('cwd-details-dropdown');

    expect(within(dropdown).getByText('repo1')).toBeInTheDocument();
    expect(within(dropdown).getByText('repo2')).toBeInTheDocument();
  });

  it('shows cwd options from the platform with repo relative cwd paths', () => {
    openCwdDropdown();

    const dropdown = screen.getByTestId('cwd-details-dropdown');

    expect(within(dropdown).getByText('repo1')).toBeInTheDocument();
    expect(within(dropdown).getByText('repo2')).toBeInTheDocument();
    expect(within(dropdown).getByText('repo2/some/subdir')).toBeInTheDocument();
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

  it('clears out saved state when changing repos', async () => {
    await clickGoto('a');

    expect(screen.getByText('sl goto --rev a')).toBeInTheDocument();

    openCwdDropdown();

    resetTestMessages();

    const dropdown = screen.getByTestId('cwd-details-dropdown');
    act(() => {
      fireEvent.click(within(dropdown).getByText('repo2'));
      simulateRepoConnected('/path/to/repo2');
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

describe('isIrrelevantToCwd', () => {
  const C = (maxPrefix: string) => COMMIT('1', 'title', '0', {maxCommonPathPrefix: maxPrefix});
  it('handles files outside cwd', () => {
    expect(isIrrelevantToCwd(C('www/'), 'fbcode/')).toBe(true);
    expect(isIrrelevantToCwd(C('www/subdir/'), 'fbcode/')).toBe(true);
  });
  it('handles files inside cwd', () => {
    expect(isIrrelevantToCwd(C('www/'), 'www/')).toBe(false);
    expect(isIrrelevantToCwd(C('www/subdir/'), 'www/')).toBe(false);
  });
  it('handles files above cwd', () => {
    expect(isIrrelevantToCwd(C('addons/isl/'), 'addons/isl')).toBe(false);
    expect(isIrrelevantToCwd(C('addons/'), 'addons/isl')).toBe(false);
    expect(isIrrelevantToCwd(C(''), 'addons/isl')).toBe(false);
  });
  it('handles cwd is root', () => {
    expect(isIrrelevantToCwd(C('addons/isl/'), '')).toBe(false);
    expect(isIrrelevantToCwd(C('addons/'), '')).toBe(false);
    expect(isIrrelevantToCwd(C(''), '')).toBe(false);
    expect(isIrrelevantToCwd(C('addons/isl/'), '/')).toBe(false);
    expect(isIrrelevantToCwd(C(''), '/')).toBe(false);
    expect(isIrrelevantToCwd(C('/'), '/')).toBe(false);
  });
});
