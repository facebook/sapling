/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  closeCommitInfoSidebar,
  simulateRepoConnected,
  simulateMessageFromServer,
} from '../testUtils';
import {CommandRunner} from '../types';
import {fireEvent, render, screen, act, waitFor} from '@testing-library/react';
import * as utils from 'shared/utils';

const allCommits = [
  COMMIT('1', 'some public base', '0', {phase: 'public'}),
  COMMIT('a', 'My Commit', '1'),
  COMMIT('b', 'Another Commit', 'a', {isDot: true}),
];

const mockNextOperationId = (id: string) =>
  jest.spyOn(utils, 'randomId').mockImplementationOnce(() => id);

describe('CommitTreeList', () => {
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
      simulateCommits({value: allCommits});
    });
  });

  it('load more button works', () => {
    fireEvent.click(screen.getByText('Load more commits'));
    expectMessageSentToServer({type: 'loadMoreCommits'});
    act(() => simulateMessageFromServer({type: 'commitsShownRange', rangeInDays: 60}));
    act(() => simulateMessageFromServer({type: 'beganLoadingMoreCommits'}));
    act(() => simulateCommits({value: allCommits}));
  });

  it('disables while running', () => {
    fireEvent.click(screen.getByText('Load more commits'));
    expectMessageSentToServer({type: 'loadMoreCommits'});
    act(() => simulateMessageFromServer({type: 'commitsShownRange', rangeInDays: 60}));
    act(() => simulateMessageFromServer({type: 'beganLoadingMoreCommits'}));
    expect(screen.getByText('Load more commits')).toBeDisabled();

    act(() => simulateCommits({value: allCommits}));
    expect(screen.getByText('Load more commits')).not.toBeDisabled();
  });

  it('uses cloud sync after loading all commits', async () => {
    fireEvent.click(screen.getByText('Load more commits'));
    expectMessageSentToServer({type: 'loadMoreCommits'});
    act(() => simulateMessageFromServer({type: 'commitsShownRange', rangeInDays: 60}));
    act(() => simulateMessageFromServer({type: 'beganLoadingMoreCommits'}));
    act(() => simulateCommits({value: allCommits}));

    fireEvent.click(screen.getByText('Load more commits'));
    expectMessageSentToServer({type: 'loadMoreCommits'});
    act(() => simulateMessageFromServer({type: 'commitsShownRange', rangeInDays: undefined}));
    act(() => simulateMessageFromServer({type: 'beganLoadingMoreCommits'}));
    act(() => simulateCommits({value: allCommits}));

    expect(screen.getByText('Fetch all cloud commits'));
    mockNextOperationId('1');
    fireEvent.click(screen.getByText('Fetch all cloud commits'));
    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: ['cloud', 'sync', '--full'],
        id: '1',
        runner: CommandRunner.Sapling,
        trackEventName: 'CommitCloudSyncOperation',
      },
    });

    act(() =>
      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'spawn',
        queue: [],
      }),
    );
    act(() =>
      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'exit',
        exitCode: 0,
        timestamp: 1234,
      }),
    );

    // buttons are gone now that we synced from cloud
    await waitFor(() => {
      expect(screen.queryByText('Load more commits')).not.toBeInTheDocument();
      expect(screen.queryByText('Fetch all cloud commits')).not.toBeInTheDocument();
    });
  });
});
