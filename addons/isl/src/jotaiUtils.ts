/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ConfigName, LocalStorageName} from './types';
import type {WritableAtom, Atom} from 'jotai';
import type {Json} from 'shared/typeUtils';

import serverAPI from './ClientToServerAPI';
import platform from './platform';
import {atom, getDefaultStore} from 'jotai';

/** A mutable atom that stores type `T`. */
export type MutAtom<T> = WritableAtom<T, [T | ((prev: T) => T)], void>;

const store = getDefaultStore();

/** Define a read-write atom backed by a config. */
export function configBackedAtom<T extends Json>(
  name: ConfigName,
  defaultValue: T,
  readonly?: false,
): MutAtom<T>;

/**
 * Define a read-only atom backed by a config.
 *
 * This can be useful for staged rollout features
 * where the config is not supposed to be set by the user.
 * (user config will override the staged rollout config)
 */
export function configBackedAtom<T extends Json>(
  name: ConfigName,
  defaultValue: T,
  readonly: true,
): Atom<T>;

export function configBackedAtom<T extends Json>(
  name: ConfigName,
  defaultValue: T,
  readonly = false,
): MutAtom<T> | Atom<T> {
  // https://jotai.org/docs/guides/persistence
  const primitiveAtom = atom<T>(defaultValue);

  let lastStrValue: undefined | string = undefined;
  serverAPI.onMessageOfType('gotConfig', event => {
    if (event.name !== name) {
      return;
    }
    lastStrValue = event.value;
    store.set(primitiveAtom, event.value === undefined ? defaultValue : JSON.parse(event.value));
  });
  serverAPI.onConnectOrReconnect(() => {
    serverAPI.postMessage({
      type: 'getConfig',
      name,
    });
  });

  return readonly
    ? atom<T>(get => get(primitiveAtom))
    : atom<T, [T | ((prev: T) => T)], void>(
        get => get(primitiveAtom),
        (get, set, update) => {
          const newValue = typeof update === 'function' ? update(get(primitiveAtom)) : update;
          set(primitiveAtom, newValue);
          const strValue = JSON.stringify(newValue);
          if (strValue !== lastStrValue) {
            lastStrValue = strValue;
            serverAPI.postMessage({
              type: 'setConfig',
              name,
              value: strValue,
            });
          }
        },
      );
}

/**
 * Loads this atom from a local persistent cache (usually browser local storage),
 * and persists any changes back to it.
 * Useful for some customizations that don't warrant a user-visible sl config,
 * for example UI expansion state.
 */
export function localStorageBackedAtom<T extends Json>(
  name: LocalStorageName,
  defaultValue: T,
): MutAtom<T> {
  const primitiveAtom = atom<T>(platform.getTemporaryState<T>(name) ?? defaultValue);

  return atom(
    get => get(primitiveAtom),
    (get, set, update) => {
      const newValue = typeof update === 'function' ? update(get(primitiveAtom)) : update;
      set(primitiveAtom, newValue);
      platform.setTemporaryState(name, newValue);
    },
  );
}

/** Perform extra operations when the atom value is changed. */
export function onAtomUpdate<T>(subscribeAtom: MutAtom<T>, onSet: (value: T) => void) {
  store.sub(subscribeAtom, () => {
    onSet(store.get(subscribeAtom));
  });
  onSet(store.get(subscribeAtom));
}

/**
 * Wraps an atom with an "onChange" callback.
 * Changing the returned atom will trigger the callback.
 * Calling this function will trigger `onChange` with the current value.
 */
export function atomWithOnChange<T>(
  originalAtom: MutAtom<T>,
  onChange: (value: T) => void,
): MutAtom<T> {
  onChange(readAtom(originalAtom));
  return atom(
    get => get(originalAtom),
    (get, set, args) => {
      const oldValue = get(originalAtom);
      set(originalAtom, args);
      const newValue = get(originalAtom);
      if (oldValue !== newValue) {
        onChange(newValue);
      }
    },
  );
}

export function readAtom<T>(atom: Atom<T>): T {
  return store.get(atom);
}

export function writeAtom<T>(atom: MutAtom<T>, value: T | ((prev: T) => T)) {
  store.set(atom, value);
}
