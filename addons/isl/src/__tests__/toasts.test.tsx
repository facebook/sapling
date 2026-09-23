/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen} from '@testing-library/react';
import {tracker} from '../analytics';
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
import {copyAndShowToast} from '../toast';

describe('toasts', () => {
  let originalActivation: PropertyDescriptor | undefined;
  let originalPolicy: PropertyDescriptor | undefined;

  beforeEach(() => {
    originalActivation = Object.getOwnPropertyDescriptor(navigator, 'userActivation');
    originalPolicy = Object.getOwnPropertyDescriptor(document, 'permissionsPolicy');
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

  afterEach(() => {
    jest.restoreAllMocks();
    if (originalActivation == null) {
      Reflect.deleteProperty(navigator, 'userActivation');
    } else {
      Reflect.defineProperty(navigator, 'userActivation', originalActivation);
    }
    if (originalPolicy == null) {
      Reflect.deleteProperty(document, 'permissionsPolicy');
    } else {
      Reflect.defineProperty(document, 'permissionsPolicy', originalPolicy);
    }
  });

  function mockClipboardContext() {
    jest.spyOn(document, 'hasFocus').mockReturnValue(true);
    Reflect.defineProperty(navigator, 'userActivation', {
      configurable: true,
      value: {hasBeenActive: true, isActive: true},
    });
    Reflect.defineProperty(document, 'permissionsPolicy', {
      configurable: true,
      value: {allowsFeature: () => true},
    });
  }

  it('shows toast when copying commit hash', async () => {
    const copySpy = jest
      .spyOn(platform, 'clipboardCopy')
      .mockImplementation(() => Promise.resolve());
    fireEvent.contextMenu(screen.getByTestId('commit-e'));
    fireEvent.click(screen.getByText('Copy Commit Hash "e"'));
    expect(await screen.findByText('Copied e')).toBeInTheDocument();
    expect(copySpy).toHaveBeenCalledWith('e');
  });

  it('tracks payload-free clipboard context after a successful write', async () => {
    const secretText = 'secret clipboard payload';
    const secretUrl = 'https://secret.example.test/D123';
    const secretHtml = `<a href="${secretUrl}">${secretText}</a>`;
    let resolveCopy: () => void = () => {};
    const clipboardResult = new Promise<void>(resolve => {
      resolveCopy = resolve;
    });
    const copySpy = jest.spyOn(platform, 'clipboardCopy').mockReturnValue(clipboardResult);
    const trackSpy = jest.spyOn(tracker, 'track').mockImplementation(() => {});
    mockClipboardContext();

    const result = copyAndShowToast(secretText, secretHtml);
    expect(trackSpy).not.toHaveBeenCalled();
    await act(async () => {
      resolveCopy();
      await result;
    });

    expect(copySpy).toHaveBeenCalledWith(secretText, secretHtml);
    const clipboardTrackCalls = trackSpy.mock.calls.filter(
      ([eventName]) => eventName === 'ClipboardCopy',
    );
    expect(clipboardTrackCalls).toEqual([
      [
        'ClipboardCopy',
        {
          extras: {
            documentHasFocus: true,
            isRich: true,
            outcome: 'success',
            permissionsPolicyAllowsClipboardWrite: true,
            userActivationHasBeenActive: true,
            userActivationIsActive: true,
            visibilityState: 'visible',
          },
        },
      ],
    ]);
    const trackedData = JSON.stringify(clipboardTrackCalls);
    expect(trackedData).not.toContain(secretText);
    expect(trackedData).not.toContain(secretHtml);
    expect(trackedData).not.toContain(secretUrl);
    expect(trackedData).not.toMatch(/"(?:text|html|url|message|stack)":/i);
  });

  it('shows an error when copying fails', async () => {
    const secretErrorMessage = 'denied secret clipboard payload';
    const copySpy = jest
      .spyOn(platform, 'clipboardCopy')
      .mockImplementation(() =>
        Promise.reject(new DOMException(secretErrorMessage, 'NotAllowedError')),
      );
    const trackSpy = jest.spyOn(tracker, 'track').mockImplementation(() => {});
    mockClipboardContext();

    fireEvent.contextMenu(screen.getByTestId('commit-e'));
    fireEvent.click(screen.getByText('Copy Commit Hash "e"'));
    expect(await screen.findByText('Could not copy e')).toBeInTheDocument();
    expect(screen.queryByText('Copied e')).not.toBeInTheDocument();
    expect(copySpy).toHaveBeenCalledWith('e');
    const clipboardTrackCalls = trackSpy.mock.calls.filter(
      ([eventName]) => eventName === 'ClipboardCopy',
    );
    expect(clipboardTrackCalls).toEqual([
      [
        'ClipboardCopy',
        {
          extras: {
            documentHasFocus: true,
            errorClass: 'NotAllowedError',
            isRich: false,
            outcome: 'native_dom_exception',
            permissionsPolicyAllowsClipboardWrite: true,
            userActivationHasBeenActive: true,
            userActivationIsActive: true,
            visibilityState: 'visible',
          },
        },
      ],
    ]);
    const trackedData = JSON.stringify(clipboardTrackCalls);
    expect(trackedData).not.toContain(secretErrorMessage);
    expect(trackedData).not.toMatch(/"(?:text|html|url|message|stack)":/i);
  });

  it('classifies a local activation rejection without logging payloads', async () => {
    const secretText = 'another secret';
    const error = new Error('secret activation failure');
    error.name = 'InactiveClipboardUserActivationError';
    jest.spyOn(platform, 'clipboardCopy').mockRejectedValue(error);
    const trackSpy = jest.spyOn(tracker, 'track').mockImplementation(() => {});
    mockClipboardContext();

    await act(async () => {
      await copyAndShowToast(secretText);
    });

    const clipboardTrackCalls = trackSpy.mock.calls.filter(
      ([eventName]) => eventName === 'ClipboardCopy',
    );
    expect(clipboardTrackCalls).toEqual([
      [
        'ClipboardCopy',
        {
          extras: {
            documentHasFocus: true,
            errorClass: 'InactiveClipboardUserActivationError',
            isRich: false,
            outcome: 'inactive_activation',
            permissionsPolicyAllowsClipboardWrite: true,
            userActivationHasBeenActive: true,
            userActivationIsActive: true,
            visibilityState: 'visible',
          },
        },
      ],
    ]);
    const trackedData = JSON.stringify(clipboardTrackCalls);
    expect(trackedData).not.toContain(secretText);
    expect(trackedData).not.toContain(error.message);
    expect(trackedData).not.toMatch(/"(?:text|html|url|message|stack)":/i);
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
    expect(copySpy).toHaveBeenCalledWith(shortHash);
  });
});
