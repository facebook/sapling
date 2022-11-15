/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FunctionComponent, PropsWithChildren} from 'react';

import {createContext, useContext, useEffect} from 'react';

/* eslint-disable no-bitwise */

type Modifiers = number;
/**
 * Modifiers for keyboard shortcuts, intended to be bitwise-OR'd together.
 * e.g. `Modifier.CMD | Modifier.CTRL`.
 */
export enum Modifier {
  NONE = 0,
  SHIFT = 1 << 0,
  CTRL = 1 << 1,
  ALT = 1 << 2,
  CMD = 1 << 3,
}

export enum KeyCode {
  Escape = 27,
  One = 49,
  Two = 50,
  Three = 51,
  Four = 52,
  Five = 53,
  A = 65,
  C = 67,
  D = 68,
  N = 78,
  P = 80,
  R = 82,
  Period = 190,
  SingleQuote = 222,
}

type CommandDefinition = [Modifiers, KeyCode];

type CommandMap<CommandName extends string> = Record<CommandName, CommandDefinition>;

function isTargetTextInputElement(event: KeyboardEvent): boolean {
  return (
    event.target != null &&
    /(vscode-text-area|vscode-text-field|textarea|input)/i.test(
      (event.target as HTMLElement).tagName,
    )
  );
}

class CommandDispatcher<CommandName extends string> extends window.EventTarget {
  private keydownListener: (event: KeyboardEvent) => void;
  constructor(commands: CommandMap<CommandName>) {
    super();
    const knownKeysWithCommands = new Set<KeyCode>();
    for (const cmdDef of Object.values(commands) as Array<CommandDefinition>) {
      const [, key] = cmdDef;
      knownKeysWithCommands.add(key);
    }
    this.keydownListener = (event: KeyboardEvent) => {
      if (!knownKeysWithCommands.has(event.keyCode)) {
        return;
      }
      if (isTargetTextInputElement(event)) {
        // we don't want shortcuts to interfere with text entry
        return;
      }
      const modValue =
        (event.shiftKey ? Modifier.SHIFT : 0) |
        (event.ctrlKey ? Modifier.CTRL : 0) |
        (event.altKey ? Modifier.ALT : 0) |
        (event.metaKey ? Modifier.CMD : 0);

      for (const [command, cmdAttrs] of Object.entries(commands) as Array<
        [CommandName, CommandDefinition]
      >) {
        const [mods, key] = cmdAttrs;
        if (key === event.keyCode && mods === modValue) {
          this.dispatchEvent(new Event(command));
          break;
        }
      }
    };
    document.body.addEventListener('keydown', this.keydownListener);
  }
}

/**
 * Add support for commands which are triggered by keyboard shortcuts.
 * return a top-level context provider which listens for global keyboard input,
 * plus a `useCommand` hook that lets you handle commands as they are dispatched.
 *
 * Commands are defined by mapping string command names to a key plus a set of modifiers.
 * CommandNames are statically known so that `useCommand` is type-safe.
 * Modifiers are a bitwise-OR union of {@link Modifier}, like `Modifier.CTRL | Modfier.CMD`
 *
 * Commands are not dispatched when the target is an input element, to ensure we don't affect typing.
 */
export function makeCommandDispatcher<CommandName extends string>(
  commands: CommandMap<CommandName>,
): [FunctionComponent<PropsWithChildren>, (command: CommandName, handler: () => void) => void] {
  const commandDispatcher = new CommandDispatcher(commands);
  const Context = createContext(commandDispatcher);

  function useCommand(command: CommandName, handler: () => void) {
    const dispatcher = useContext(Context);

    // register & unregister the event listener while the component is mounted
    useEffect(() => {
      dispatcher.addEventListener(command, handler);
      return () => dispatcher.removeEventListener(command, handler);
    }, [command, handler, dispatcher]);
  }

  return [
    ({children}) => <Context.Provider value={commandDispatcher}>{children}</Context.Provider>,
    useCommand,
  ];
}
