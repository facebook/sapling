/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ConfigName} from './types';
import type {AtomEffect} from 'recoil';
import type {Json} from 'shared/typeUtils';

import serverAPI from './ClientToServerAPI';

/**
 * Loads this atom from server config via `sl config`,
 * and persists any changes via `sl config --user name value`
 */
export function persistAtomToConfigEffect<T extends Json>(name: ConfigName): AtomEffect<T> {
  return ({onSet, setSelf}) => {
    onSet(newValue => {
      serverAPI.postMessage({
        type: 'setConfig',
        name,
        value: JSON.stringify(newValue),
      });
    });
    serverAPI.onMessageOfType('gotConfig', event => {
      if (event.value != null) {
        setSelf(JSON.parse(event.value));
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
