/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen, waitFor} from '@testing-library/react';
import * as utils from 'shared/utils';
import App from '../App';
import {
  COMMIT,
  closeCommitInfoSidebar,
  expectMessageSentToServer,
  getLastMessageOfTypeSentToServer,
  simulateCommits,
  simulateMessageFromServer,
  simulateRepoConnected,
} from '../testUtils';
import {CommandRunner} from '../types';

const allCommits = [
  COMMIT('1', 'some public base', '0', {phase: 'public'}),
  COMMIT('a', 'My Commit', '1'),
  COMMIT('b', 'Another Commit', 'a', {isDot: true}),
];

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

    expectMessageSentToServer({type: 'getConfig', name: 'extensions.commitcloud'});
    act(() =>
      simulateMessageFromServer({type: 'gotConfig', name: 'extensions.commitcloud', value: ''}),
    );

    await waitFor(() => expect(screen.getByText('Fetch all cloud commits')));
    fireEvent.click(screen.getByText('Fetch all cloud commits'));

    const message = await waitFor(() =>
      utils.nullthrows(getLastMessageOfTypeSentToServer('runOperation')),
    );
    const id = message.operation.id;

    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: ['cloud', 'sync', '--full'],
        id,
        runner: CommandRunner.Sapling,
        trackEventName: 'CommitCloudSyncOperation',
      },
    });

    act(() =>
      simulateMessageFromServer({
        type: 'operationProgress',
        id,
        kind: 'spawn',
        queue: [],
      }),
    );
    act(() =>
      simulateMessageFromServer({
        type: 'operationProgress',
        id,
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

  it('does not show cloud sync button if commit cloud not enabled', async () => {
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

    expectMessageSentToServer({type: 'getConfig', name: 'extensions.commitcloud'});
    // eslint-disable-next-line require-await
    await act(async () =>
      simulateMessageFromServer({type: 'gotConfig', name: 'extensions.commitcloud', value: '!'}),
    );

    expect(screen.queryByText('Fetch all cloud commits')).not.toBeInTheDocument();
  });
});
