/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useMemo} from 'react';
import {makeCommandDispatcher, KeyCode, Modifier} from 'shared/KeyboardShortcuts';
import {TypedEventEmitter} from 'shared/TypedEventEmitter';

/* eslint-disable no-bitwise */
export const [ISLCommandContext, useCommand, dispatchCommand] = makeCommandDispatcher({
  ToggleSidebar: [Modifier.CMD, KeyCode.Period],
  OpenUncommittedChangesComparisonView: [Modifier.CMD, KeyCode.SingleQuote],
  OpenHeadChangesComparisonView: [Modifier.CMD | Modifier.SHIFT, KeyCode.SingleQuote],
  Escape: [Modifier.NONE, KeyCode.Escape],
  SelectUpwards: [Modifier.NONE, KeyCode.UpArrow],
  SelectDownwards: [Modifier.NONE, KeyCode.DownArrow],
  ContinueSelectionUpwards: [Modifier.SHIFT, KeyCode.UpArrow],
  ContinueSelectionDownwards: [Modifier.SHIFT, KeyCode.DownArrow],
  HideSelectedCommits: [Modifier.NONE, KeyCode.Backspace],
  ToggleTheme: [Modifier.ALT, KeyCode.T],
  ToggleShelvedChangesDropdown: [Modifier.ALT, KeyCode.S],
  ToggleDownloadCommitsDropdown: [Modifier.ALT, KeyCode.D],
  ToggleCwdDropdown: [Modifier.ALT, KeyCode.C],
});

export type ISLCommandName = Parameters<typeof useCommand>[0];

/** Like useCommand, but returns an eventEmitter you can subscribe to */
export function useCommandEvent(commandName: ISLCommandName): TypedEventEmitter<'change', null> {
  const emitter = useMemo(() => new TypedEventEmitter<'change', null>(), []);
  useCommand(commandName, () => {
    emitter.emit('change', null);
  });
  return emitter;
}
