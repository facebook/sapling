/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import foundPlatform from '../platform';
import {
  TEST_COMMIT_HISTORY,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
} from '../testUtils';
import {screen, act, fireEvent, render} from '@testing-library/react';

describe('toasts', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: TEST_COMMIT_HISTORY,
      });
    });
  });

  it('shows toast when copying commit hash', () => {
    const copySpy = jest.spyOn(foundPlatform, 'clipboardCopy').mockImplementation(() => {});
    fireEvent.contextMenu(screen.getByTestId('commit-e'));
    fireEvent.click(screen.getByText('Copy Commit Hash "e"'));
    expect(screen.getByText('Copied e')).toBeInTheDocument();
    expect(copySpy).toHaveBeenCalledWith('e', undefined);
  });
});
