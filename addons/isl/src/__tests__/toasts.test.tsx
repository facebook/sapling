/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen} from '@testing-library/react';
import App from '../App';
import {copyCommitHashFormatAtom} from '../Commit';
import {writeAtom} from '../jotaiUtils';
import platform from '../platform';
import {
  COMMIT,
  TEST_COMMIT_HISTORY,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
} from '../testUtils';

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

  it('shows toast when copying commit hash', async () => {
    const copySpy = jest
      .spyOn(platform, 'clipboardCopy')
      .mockImplementation(() => Promise.resolve());
    fireEvent.contextMenu(screen.getByTestId('commit-e'));
    fireEvent.click(screen.getByText('Copy Commit Hash "e"'));
    expect(await screen.findByText('Copied e')).toBeInTheDocument();
    expect(copySpy).toHaveBeenCalledWith('e', undefined);
  });

  it('shows an error when copying fails', async () => {
    const copySpy = jest
      .spyOn(platform, 'clipboardCopy')
      .mockImplementation(() => Promise.reject(new DOMException('denied', 'NotAllowedError')));
    fireEvent.contextMenu(screen.getByTestId('commit-e'));
    fireEvent.click(screen.getByText('Copy Commit Hash "e"'));
    expect(await screen.findByText('Could not copy e')).toBeInTheDocument();
    expect(screen.queryByText('Copied e')).not.toBeInTheDocument();
    expect(copySpy).toHaveBeenCalledWith('e', undefined);
  });

  it('copies short hash when setting is configured', async () => {
    const longHash = 'abcdef1234567890abcdef';
    const shortHash = longHash.slice(0, 12);
    act(() => {
      simulateCommits({
        value: [...TEST_COMMIT_HISTORY, COMMIT(longHash, 'Commit With Long Hash', '1')],
      });
      writeAtom(copyCommitHashFormatAtom, 'short');
    });
    const copySpy = jest
      .spyOn(platform, 'clipboardCopy')
      .mockImplementation(() => Promise.resolve());
    fireEvent.contextMenu(screen.getByTestId(`commit-${longHash}`));
    fireEvent.click(screen.getByText(`Copy Commit Hash "${shortHash}"`));
    expect(await screen.findByText(`Copied ${shortHash}`)).toBeInTheDocument();
    expect(copySpy).toHaveBeenCalledWith(shortHash, undefined);
  });
});
