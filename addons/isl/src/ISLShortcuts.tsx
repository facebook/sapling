/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {makeCommandDispatcher, KeyCode, Modifier} from 'shared/KeyboardShortcuts';

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
});

export type ISLCommandName = Parameters<typeof useCommand>[0];
