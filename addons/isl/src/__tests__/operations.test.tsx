/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';

import App from '../App';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  closeCommitInfoSidebar,
  simulateMessageFromServer,
  TEST_COMMIT_HISTORY,
  simulateRepoConnected,
} from '../testUtils';
import {fireEvent, render, screen, within} from '@testing-library/react';
import {act} from 'react-dom/test-utils';
import * as utils from 'shared/utils';

jest.mock('../MessageBus');

const clickGoto = (commit: Hash) => {
  const myCommit = screen.queryByTestId(`commit-${commit}`);
  const gotoButton = myCommit?.querySelector('.goto-button button');
  expect(gotoButton).toBeDefined();
  fireEvent.click(gotoButton as Element);
};

describe('operations', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      closeCommitInfoSidebar();
      expectMessageSentToServer({
        type: 'subscribeSmartlogCommits',
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

  it('shows running operation', () => {
    clickGoto('c');

    expect(
      within(screen.getByTestId('progress-container')).getByText('sl goto --rev c'),
    ).toBeInTheDocument();
  });

  it('shows stdout from running command', () => {
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

  it('shows successful exit status', () => {
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
      });
    });

    expect(screen.queryByLabelText('Command exited successfully')).toBeInTheDocument();
    expect(
      within(screen.getByTestId('progress-container')).getByText('sl goto --rev c'),
    ).toBeInTheDocument();
  });

  it('shows unsuccessful exit status', () => {
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
      });
    });

    expect(screen.queryByLabelText('Command exited unsuccessfully')).toBeInTheDocument();
    expect(
      within(screen.getByTestId('progress-container')).getByText('sl goto --rev c'),
    ).toBeInTheDocument();
  });

  describe('queued commands', () => {
    it('optimistically shows queued commands', () => {
      clickGoto('c');

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'spawn',
          queue: [],
        });
      });

      clickGoto('a');
      clickGoto('b');

      expect(
        within(screen.getByTestId('queued-commands')).getByText('sl goto --rev a'),
      ).toBeInTheDocument();
      expect(
        within(screen.getByTestId('queued-commands')).getByText('sl goto --rev b'),
      ).toBeInTheDocument();
    });

    it('dequeues when the server starts the next command', () => {
      clickGoto('c');

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'spawn',
          queue: [],
        });
      });

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
      clickGoto('c'); // id 1

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'spawn',
          queue: [],
        });
      });

      clickGoto('a'); // id 2
      clickGoto('b'); // id 3

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'exit',
          exitCode: 0,
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
      clickGoto('c');

      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'spawn',
          queue: [],
        });
      });

      clickGoto('a');
      clickGoto('b');

      expect(screen.queryByTestId('queued-commands')).toBeInTheDocument();
      act(() => {
        // original goto fails
        simulateMessageFromServer({
          type: 'operationProgress',
          id: '1',
          kind: 'exit',
          exitCode: -1,
        });
      });
      expect(screen.queryByTestId('queued-commands')).not.toBeInTheDocument();
    });
  });
});
