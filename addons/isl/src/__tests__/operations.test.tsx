/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {mostRecentSubscriptionIds} from '../serverAPIState';
import {CommitTreeListTestUtils} from '../testQueries';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  closeCommitInfoSidebar,
  simulateMessageFromServer,
  TEST_COMMIT_HISTORY,
  simulateRepoConnected,
  dragAndDropCommits,
  COMMIT,
} from '../testUtils';
import {fireEvent, render, screen, within} from '@testing-library/react';
import {act} from 'react-dom/test-utils';
import * as utils from 'shared/utils';

jest.mock('../MessageBus');

const {clickGoto} = CommitTreeListTestUtils;

const abortButton = () => screen.queryByTestId('abort-button');

describe('operations', () => {
  beforeEach(() => {
    jest.useFakeTimers();
    resetTestMessages();
    render(<App />);
    act(() => {
      closeCommitInfoSidebar();
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateRepoConnected();
      simulateCommits({
        value: TEST_COMMIT_HISTORY,
      });
    });

    // ensure operations have predictable ID
    jest
      .spyOn(utils, 'randomId')
      .mockImplementationOnce(() => '1')
      .mockImplementationOnce(() => '2')
      .mockImplementationOnce(() => '3')
      .mockImplementationOnce(() => '4');
  });

  const mockNextOperationId = (id: string) =>
    jest.spyOn(utils, 'randomId').mockImplementationOnce(() => id);

  afterEach(() => {
    jest.useRealTimers();
    jest.spyOn(utils, 'randomId').mockRestore();
  });

  it('shows running operation', () => {
    mockNextOperationId('1');
    clickGoto('c');

    expect(
      within(screen.getByTestId('progress-container')).getByText('sl goto --rev c'),
    ).toBeInTheDocument();
  });

  it('shows stdout from running command', () => {
    mockNextOperationId('1');
    clickGoto('c');

    act(() => {
      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'spawn',
        queue: [],
      });

      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'stdout',
        message: 'some progress...',
      });
    });

    expect(screen.queryByText('some progress...')).toBeInTheDocument();

    act(() => {
      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'stdout',
        message: 'another message',
      });
    });

    expect(screen.queryByText('another message')).toBeInTheDocument();
  });

  it('shows stderr from running command', () => {
    mockNextOperationId('1');
    clickGoto('c');

    act(() => {
      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'spawn',
        queue: [],
      });

      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'stderr',
        message: 'some progress...',
      });
    });

    expect(screen.queryByText('some progress...')).toBeInTheDocument();

    act(() => {
      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'stderr',
        message: 'another message',
      });
    });

    expect(screen.queryByText('another message')).toBeInTheDocument();
  });

  it('shows abort on long-running commands', () => {
    mockNextOperationId('1');
    clickGoto('c');
    expect(abortButton()).toBeNull();

    act(() => {
      jest.advanceTimersByTime(600000);
    });
    expect(abortButton()).toBeInTheDocument();
  });

  it('shows successful exit status', () => {
    mockNextOperationId('1');
    clickGoto('c');

    act(() => {
      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'spawn',
        queue: [],
      });

      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'exit',
        exitCode: 0,
        timestamp: 1234,
      });
    });

    expect(screen.queryByLabelText('Command exited successfully')).toBeInTheDocument();
    expect(
      within(screen.getByTestId('progress-container')).getByText('sl goto --rev c'),
    ).toBeInTheDocument();
  });

  it('shows unsuccessful exit status', () => {
    mockNextOperationId('1');
    clickGoto('c');

    act(() => {
      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'spawn',
        queue: [],
      });

      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'exit',
        exitCode: -1,
        timestamp: 1234,
      });
    });

    expect(screen.queryByLabelText('Command exited unsuccessfully')).toBeInTheDocument();
    expect(
      within(screen.getByTestId('progress-container')).getByText('sl goto --rev c'),
    ).toBeInTheDocument();
  });

  it('reacts to abort', () => {
    mockNextOperationId('1');
    clickGoto('c');
    act(() => {
      jest.advanceTimersByTime(600000);
    });

    // Start abort
    fireEvent.click(abortButton() as Element);

    // During abort
    expect(abortButton()).toBeDisabled();

    // After abort (process exit)
    act(() => {
      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'exit',
        exitCode: 130,
        timestamp: 1234,
      });
    });
    expect(abortButton()).toBeNull();
    expect(screen.queryByLabelText('Command aborted')).toBeInTheDocument();
  });

  describe('queued commands', () => {
    it('optimistically shows queued commands', () => {
      mockNextOperationId('1');
      clickGoto('c');

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'spawn',
          queue: [],
        });
      });

      mockNextOperationId('2');
      clickGoto('a');
      mockNextOperationId('3');
      clickGoto('b');

      expect(
        within(screen.getByTestId('queued-commands')).getByText('sl goto --rev a'),
      ).toBeInTheDocument();
      expect(
        within(screen.getByTestId('queued-commands')).getByText('sl goto --rev b'),
      ).toBeInTheDocument();
    });

    it('dequeues when the server starts the next command', () => {
      mockNextOperationId('1');
      clickGoto('c');

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'spawn',
          queue: [],
        });
      });

      mockNextOperationId('2');
      clickGoto('a');
      expect(
        within(screen.getByTestId('queued-commands')).getByText('sl goto --rev a'),
      ).toBeInTheDocument();

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '2',
          kind: 'spawn',
          queue: [],
        });
      });

      expect(screen.queryByTestId('queued-commands')).not.toBeInTheDocument();
    });

    it('takes queued command info from server', () => {
      mockNextOperationId('1');
      clickGoto('c');

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'spawn',
          queue: [],
        });
      });

      mockNextOperationId('2');
      clickGoto('a');
      mockNextOperationId('3');
      clickGoto('b');

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'exit',
          exitCode: 0,
          timestamp: 1234,
        });
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '2',
          kind: 'spawn',
          queue: ['3'],
        });
      });

      expect(
        within(screen.getByTestId('queued-commands')).getByText('sl goto --rev b'),
      ).toBeInTheDocument();
      expect(
        within(screen.getByTestId('queued-commands')).queryByText('sl goto --rev a'),
      ).not.toBeInTheDocument();
    });

    it('error running command cancels queued commands', () => {
      mockNextOperationId('1');
      clickGoto('c');

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'spawn',
          queue: [],
        });
      });

      mockNextOperationId('2');
      clickGoto('a');
      mockNextOperationId('3');
      clickGoto('b');

      expect(screen.queryByTestId('queued-commands')).toBeInTheDocument();
      act(() => {
        // original goto fails
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'exit',
          exitCode: -1,
          timestamp: 1234,
        });
      });
      expect(screen.queryByTestId('queued-commands')).not.toBeInTheDocument();
    });

    it('force clears optimistic state after fetching after an operation has finished', () => {
      const commitsBeforeOperations = {
        value: [
          COMMIT('e', 'Commit E', 'd', {isHead: true}),
          COMMIT('d', 'Commit D', 'c'),
          COMMIT('c', 'Commit C', 'b'),
          COMMIT('b', 'Commit B', 'a'),
          COMMIT('a', 'Commit A', '1'),
          COMMIT('1', 'public', '0', {phase: 'public'}),
        ],
      };
      const commitsAfterOperations = {
        value: [
          COMMIT('e2', 'Commit E', 'd2'),
          COMMIT('d2', 'Commit D', 'c2', {isHead: true}), // goto
          COMMIT('c2', 'Commit C', 'a'), // rebased
          COMMIT('b', 'Commit B', 'a'),
          COMMIT('a', 'Commit A', '1'),
          COMMIT('1', 'public', '0', {phase: 'public'}),
        ],
      };

      act(() =>
        simulateMessageFromServer({
          type: 'subscriptionResult',
          kind: 'smartlogCommits',
          subscriptionID: mostRecentSubscriptionIds.smartlogCommits,
          data: {
            fetchStartTimestamp: 1,
            fetchCompletedTimestamp: 2,
            commits: commitsBeforeOperations,
          },
        }),
      );

      //  100     200      300      400      500      600      700
      //  |--------|--------|--------|--------|--------|--------|
      //  <----- rebase ---->
      //  ...................<----- goto ----->
      //                                 <----fetch1--->  (no effect)
      //                                            <---fetch2-->   (clears optimistic state)

      // t=100 simulate spawn rebase
      // t=200 simulate queue goto
      // t=300 simulate exit rebase
      //       expect optimistic "You were here..."
      // t=400 simulate spawn goto
      // t=500 simulate exit goto
      //       expect optimistic "You were here..."
      // t=600 simulate new commits fetch started @ t=450, with new head
      //       no effect
      // t=700 simulate new commits fetch started @ t=550, with new head
      // BEFORE: Optimistic state wouldn't resolve, so "You were here..." would stick
      // AFTER: Optimistic state forced to resolve, so "You were here..." is gone

      dragAndDropCommits('c', 'a');
      fireEvent.click(screen.getByText('Run Rebase'));
      clickGoto('d'); // checkout d, which is now optimistic from the rebase, since it'll actually become d2.

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
          id: '2',
          kind: 'queue',
          queue: ['2'],
        }),
      );
      act(() =>
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'exit',
          exitCode: 0,
          timestamp: 300,
        }),
      );
      act(() =>
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '2',
          kind: 'spawn',
          queue: [],
        }),
      );
      act(() =>
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '2',
          kind: 'exit',
          exitCode: 0,
          timestamp: 500,
        }),
      );
      act(() =>
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '2',
          kind: 'exit',
          exitCode: 0,
          timestamp: 500,
        }),
      );

      act(() =>
        simulateMessageFromServer({
          type: 'subscriptionResult',
          kind: 'smartlogCommits',
          subscriptionID: mostRecentSubscriptionIds.smartlogCommits,
          data: {
            commits: commitsBeforeOperations, // not observed the new head
            fetchStartTimestamp: 400, // before goto finished
            fetchCompletedTimestamp: 450,
          },
        }),
      );

      // this latest fetch started before the goto finished, so we don't know that it has all the information
      // included. So the optimistic state remains.
      expect(screen.getByText('You were here...')).toBeInTheDocument();

      act(() =>
        simulateMessageFromServer({
          type: 'subscriptionResult',
          kind: 'smartlogCommits',
          subscriptionID: mostRecentSubscriptionIds.smartlogCommits,
          data: {
            commits: commitsAfterOperations, // observed the new head
            fetchStartTimestamp: 400, // before goto finished
            fetchCompletedTimestamp: 450,
          },
        }),
      );

      // However, even if the latest fetch started before the goto finished,
      // if "goto" saw that head = the new commit, the optimistic state is a
      // no-op and we won't see "You were here...".
      expect(screen.queryByText('You were here...')).not.toBeInTheDocument();

      act(() =>
        simulateMessageFromServer({
          type: 'subscriptionResult',
          kind: 'smartlogCommits',
          subscriptionID: mostRecentSubscriptionIds.smartlogCommits,
          data: {
            commits: commitsBeforeOperations, // intentionally "incorrect" to test the force clear out
            fetchStartTimestamp: 550, // after goto finished
            fetchCompletedTimestamp: 600,
          },
        }),
      );

      // This latest fetch started AFTER the goto finished, so we can be sure
      // it accounts for that operation.
      // So the optimistic state should be cleared out, even though we didn't
      // detect that the optimistic state should have resolved according to the applier.
      expect(screen.queryByText('You were here...')).not.toBeInTheDocument();
    });
  });

  describe('progress messages', () => {
    it('shows progress messages', () => {
      mockNextOperationId('1');
      clickGoto('c');

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'spawn',
          queue: [],
        });

        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'progress',
          progress: {message: 'doing the thing', progress: 3, progressTotal: 7},
        });
      });

      expect(
        within(screen.getByTestId('progress-container')).getByText('doing the thing'),
      ).toBeInTheDocument();
    });

    it('hide progress on new stdout', () => {
      mockNextOperationId('1');
      clickGoto('c');

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'spawn',
          queue: [],
        });

        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'progress',
          progress: {message: 'doing the thing'},
        });
      });

      expect(
        within(screen.getByTestId('progress-container')).getByText('doing the thing'),
      ).toBeInTheDocument();

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'stdout',
          message: 'hello',
        });
      });

      expect(
        within(screen.getByTestId('progress-container')).queryByText('doing the thing'),
      ).not.toBeInTheDocument();
      expect(
        within(screen.getByTestId('progress-container')).getByText('hello'),
      ).toBeInTheDocument();
    });
  });

  describe('inline progress', () => {
    it('shows progress messages next to commits', () => {
      mockNextOperationId('1');
      clickGoto('c');

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'spawn',
          queue: [],
        });

        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'inlineProgress',
          hash: 'c',
          message: 'going...', // not a real thing for goto operation, but we support arbitrary progress
        });
      });

      expect(
        within(screen.getByTestId('commit-tree-root')).getByText('going...'),
      ).toBeInTheDocument();
    });
  });
});
