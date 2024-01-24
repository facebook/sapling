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
import {RateLimiter} from 'shared/RateLimiter';
import {isPromise} from 'shared/utils';

/** A mutable atom that stores type `T`. */
export type MutAtom<T> = WritableAtom<T, [T | ((prev: T) => T)], void>;

/**
 * The store being used. Do not use this directly. Alternatives are:
 * - use `readAtom` instead of `store.get`.
 * - use `writeAtom` instead of `store.set`.
 * - use `atomWithOnChange` instead of `store.sub`.
 */
let store = getDefaultStore();

/**
 * Replace the current Jotai store used by this module.
 * Practically, this is only useful for tests to reset states.
 */
export function setJotaiStore(newStore: typeof store) {
  store = newStore;
}

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
    writeAtom(primitiveAtom, event.value === undefined ? defaultValue : JSON.parse(event.value));
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

/**
 * Creates a lazily initialized atom.
 * On first read, trigger `load` to get the actual value.
 * `fallback` provides the value when the async `load` is running.
 * `original` is an optioinal nullable atom to provide the value.
 */
export function lazyAtom<T>(
  load: () => Promise<T> | T,
  fallback: T,
  original?: MutAtom<T | undefined>,
): MutAtom<T> {
  const originalAtom = original ?? atom<T | undefined>(undefined);
  const limiter = new RateLimiter(1);
  return atom(
    get => {
      const value = get(originalAtom);
      if (value !== undefined) {
        return value;
      }
      const loaded = load();
      if (!isPromise(loaded)) {
        writeAtom(originalAtom, loaded);
        return loaded;
      }
      // Kick off the "load" but rate limit it.
      limiter.enqueueRun(async () => {
        if (get(originalAtom) !== undefined) {
          // A previous "load" was completed.
          return;
        }
        const newValue = await loaded;
        writeAtom(originalAtom, newValue);
      });
      // Use the fallback value while waiting for the promise.
      return fallback;
    },
    (get, set, args) => {
      const newValue =
        typeof args === 'function' ? (args as (prev: T) => T)(get(originalAtom) ?? fallback) : args;
      set(originalAtom, newValue);
    },
  );
}

export function readAtom<T>(atom: Atom<T>): T {
  return store.get(atom);
}

export function writeAtom<T>(atom: MutAtom<T>, value: T | ((prev: T) => T)) {
  store.set(atom, value);
}

// Once we are pure Jotai, consider adding a `cwd` atom then update `resetOnCwdChange`
// to be something that depends on the `cwd` atom.
export function resetOnCwdChange<T>(atom: WritableAtom<T, [T], unknown>, defaultValue: T) {
  serverAPI.onCwdChanged(() => {
    store.set(atom, defaultValue);
  });
}
