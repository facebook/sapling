/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ConfigName} from './types';
import type {WritableAtom, Atom} from 'jotai';
import type {Json} from 'shared/typeUtils';

import serverAPI from './ClientToServerAPI';
import {atom, getDefaultStore} from 'jotai';

const store = getDefaultStore();

/** Define a read-write atom backed by a config. */
export function configBackedAtom<T extends Json>(
  name: ConfigName,
  defaultValue: T,
  readonly?: false,
): WritableAtom<T, [T], void>;

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
): WritableAtom<T, [T], void> | Atom<T> {
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
    : atom<T, [T], void>(
        get => get(primitiveAtom),
        (_get, set, newValue) => {
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
