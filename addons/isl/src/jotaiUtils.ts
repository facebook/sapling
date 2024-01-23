/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ConfigName} from './types';
import type {WritableAtom} from 'jotai';
import type {Json} from 'shared/typeUtils';

import serverAPI from './ClientToServerAPI';
import {atom, getDefaultStore} from 'jotai';

const store = getDefaultStore();

export function configBackedAtom<T extends Json>(
  name: ConfigName,
  defaultValue: T,
): WritableAtom<T, [T], void> {
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

  return atom<T, [T], void>(
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
