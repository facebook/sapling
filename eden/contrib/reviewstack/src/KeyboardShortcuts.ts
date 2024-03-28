/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {makeCommandDispatcher, KeyCode, Modifier} from 'shared/KeyboardShortcuts';

export const [ShortcutCommandContext, useCommand] = makeCommandDispatcher({
  ToggleSidebar: [Modifier.CMD, KeyCode.Period],
  NextInStack: [Modifier.CMD, KeyCode.DownArrow],
  PreviousInStack: [Modifier.CMD, KeyCode.UpArrow],
  Approve: [Modifier.ALT, KeyCode.A],
  Comment: [Modifier.ALT, KeyCode.C],
  RequestChanges: [Modifier.ALT, KeyCode.R],
});
