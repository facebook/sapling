/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {KeyCode, Modifier} from 'shared/KeyboardShortcuts';

import './Kbd.css';

/** Keyboard key, useful for rendering keyboard shortcuts */
export function Kbd({keycode, modifiers}: {keycode: KeyCode; modifiers?: Array<Modifier>}) {
  return (
    <span className="kbd-group" title={asEntireString(keycode, modifiers)}>
      {modifiers
        ?.filter(modifier => modifier != Modifier.NONE)
        .map(modifier => (
          <kbd className="modifier" key={modifier}>
            {modifierToIcon[modifier]}
          </kbd>
        ))}
      <kbd>{keycodeToString(keycode)}</kbd>
    </span>
  );
}

function keycodeToString(keycode: KeyCode): string {
  switch (keycode) {
    case KeyCode.QuestionMark:
      return '?';
    case KeyCode.SingleQuote:
      return "'";
    case KeyCode.Period:
      return '.';
    case KeyCode.Escape:
      return 'Esc';
    case KeyCode.Plus:
      return '+';
    case KeyCode.Minus:
      return '-';
    default:
      return String.fromCharCode(keycode).toUpperCase();
  }
}

const modifierToIcon = {
  [Modifier.ALT]: '⌥',
  [Modifier.CMD]: '⌘',
  [Modifier.SHIFT]: '⇧',
  [Modifier.CTRL]: '⌃',
  [Modifier.NONE]: '',
} as const;

const modifierToString = {
  [Modifier.ALT]: 'Alt',
  [Modifier.CMD]: 'Command',
  [Modifier.SHIFT]: 'Shift',
  [Modifier.CTRL]: 'Control',
  [Modifier.NONE]: '',
} as const;

function asEntireString(keycode: KeyCode, modifiers?: Array<Modifier>): string {
  const result: Array<string> = [];
  for (const modifier of modifiers ?? []) {
    result.push(modifierToString[modifier]);
  }
  result.push(keycodeToString(keycode));
  return result.join(' + ');
}
