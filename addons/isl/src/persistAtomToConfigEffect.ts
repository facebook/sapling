/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ConfigName, LocalStorageName} from './types';
import type {AtomEffect} from 'recoil';
import type {Json} from 'shared/typeUtils';

import serverAPI from './ClientToServerAPI';
import platform from './platform';

/**
 * Loads this atom from server config via `sl config`,
 * and persists any changes via `sl config --user name value`
 */
export function persistAtomToConfigEffect<T extends Json>(
  name: ConfigName,
  defaultValue?: T,
): AtomEffect<T> {
  return ({onSet, setSelf}) => {
    onSet(newValue => {
      serverAPI.postMessage({
        type: 'setConfig',
        name,
        value: JSON.stringify(newValue),
      });
    });
    serverAPI.onMessageOfType('gotConfig', event => {
      if (event.name !== name) {
        return;
      }
      if (event.value != null) {
        setSelf(JSON.parse(event.value));
      } else if (defaultValue != null) {
        setSelf(defaultValue);
      }
    });
    serverAPI.onConnectOrReconnect(() => {
      serverAPI.postMessage({
        type: 'getConfig',
        name,
      });
    });
  };
}

/**
 * Loads this atom from a local persistent cache (usually browser local storage),
 * and persists any changes back to it.
 * Useful for some customizations that don't warrant a user-visible sl config,
 * for example UI expansion state.
 */
export function persistAtomToLocalStorageEffect<T extends Json>(
  name: LocalStorageName,
): AtomEffect<T> {
  return ({onSet, setSelf}) => {
    onSet(newValue => {
      platform.setTemporaryState(name, newValue);
    });
    const found = platform.getTemporaryState<T>(name);
    if (found != null) {
      setSelf(found);
    }
  };
}
