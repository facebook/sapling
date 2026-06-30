/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen} from '@testing-library/react';
import {KeyCode, Modifier} from 'isl-components/KeyboardShortcuts';
import App from '../App';
import {dispatchCommand} from '../ISLShortcuts';
import {
  closeCommitInfoSidebar,
  expectMessageNOTSentToServer,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
  simulateRepoConnected,
  TEST_COMMIT_HISTORY,
} from '../testUtils';

describe('keyboard shortcut overrides', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      simulateRepoConnected();
      closeCommitInfoSidebar();
      simulateCommits({value: TEST_COMMIT_HISTORY});
    });
  });

  function applyOverrides(overrides: object) {
    act(() => {
      simulateMessageFromServer({
        type: 'gotConfig',
        name: 'isl.keyboard-shortcut-overrides',
        value: JSON.stringify(overrides),
      });
    });
  }

  it('rebinds Pull to the overridden key and disables the default', () => {
    applyOverrides({Pull: [[Modifier.ALT], KeyCode.G]});
    resetTestMessages();

    // the default Alt+P no longer pulls
    act(() => {
      fireEvent.keyDown(document.body, {keyCode: KeyCode.P, altKey: true});
    });
    expectMessageNOTSentToServer(
      expect.objectContaining({
        type: 'runOperation',
        operation: expect.objectContaining({trackEventName: 'PullOperation'}),
      }),
    );

    // the overridden Alt+G now pulls
    act(() => {
      fireEvent.keyDown(document.body, {keyCode: KeyCode.G, altKey: true});
    });
    expectMessageSentToServer(
      expect.objectContaining({
        type: 'runOperation',
        operation: expect.objectContaining({args: ['pull'], trackEventName: 'PullOperation'}),
      }),
    );
  });

  it('restores the default binding when the override is cleared', () => {
    applyOverrides({Pull: [[Modifier.ALT], KeyCode.G]});
    applyOverrides({});
    resetTestMessages();

    act(() => {
      fireEvent.keyDown(document.body, {keyCode: KeyCode.P, altKey: true});
    });
    expectMessageSentToServer(
      expect.objectContaining({
        type: 'runOperation',
        operation: expect.objectContaining({args: ['pull'], trackEventName: 'PullOperation'}),
      }),
    );
  });

  it('rebinds Pull via the Keyboard Shortcuts dialog and persists to config', () => {
    act(() => {
      dispatchCommand('OpenShortcutHelp');
    });
    expect(screen.getByTestId('edit-shortcut-Pull')).toBeInTheDocument();

    act(() => {
      fireEvent.click(screen.getByTestId('edit-shortcut-Pull'));
    });
    act(() => {
      fireEvent.keyDown(screen.getByTestId('shortcut-capture'), {keyCode: KeyCode.G, altKey: true});
    });

    expectMessageSentToServer(
      expect.objectContaining({
        type: 'setConfig',
        name: 'isl.keyboard-shortcut-overrides',
        value: JSON.stringify({Pull: [[Modifier.ALT], KeyCode.G]}),
      }),
    );
  });

  it('does not bind reserved keys (e.g. Tab) when capturing a shortcut', () => {
    act(() => {
      dispatchCommand('OpenShortcutHelp');
    });
    act(() => {
      fireEvent.click(screen.getByTestId('edit-shortcut-Pull'));
    });
    resetTestMessages();
    // Tab (keyCode 9) is reserved for navigation and must not be captured as a binding.
    act(() => {
      fireEvent.keyDown(screen.getByTestId('shortcut-capture'), {keyCode: 9});
    });
    expectMessageNOTSentToServer(
      expect.objectContaining({type: 'setConfig', name: 'isl.keyboard-shortcut-overrides'}),
    );
    // Still in capture mode; a real key still binds. (Use a distinct key from other tests so the
    // config-atom's value-dedupe doesn't suppress the setConfig message.)
    act(() => {
      fireEvent.keyDown(screen.getByTestId('shortcut-capture'), {keyCode: KeyCode.N, altKey: true});
    });
    expectMessageSentToServer(
      expect.objectContaining({
        type: 'setConfig',
        name: 'isl.keyboard-shortcut-overrides',
        value: JSON.stringify({Pull: [[Modifier.ALT], KeyCode.N]}),
      }),
    );
  });
});
