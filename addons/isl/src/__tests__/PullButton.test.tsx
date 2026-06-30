/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, render} from '@testing-library/react';
import App from '../App';
import {dispatchCommand} from '../ISLShortcuts';
import {
  closeCommitInfoSidebar,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateRepoConnected,
  TEST_COMMIT_HISTORY,
} from '../testUtils';
import {CommandRunner} from '../types';

describe('PullButton keyboard shortcut', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      simulateRepoConnected();
      closeCommitInfoSidebar();
      simulateCommits({value: TEST_COMMIT_HISTORY});
    });
  });

  it('runs Pull on the Pull command', () => {
    act(() => {
      dispatchCommand('Pull');
    });

    expectMessageSentToServer(
      expect.objectContaining({
        type: 'runOperation',
        operation: expect.objectContaining({
          args: ['pull'],
          runner: CommandRunner.Sapling,
          trackEventName: 'PullOperation',
        }),
      }),
    );
  });
});
