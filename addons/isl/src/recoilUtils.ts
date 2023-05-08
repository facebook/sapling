/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AtomEffect} from 'recoil';

import serverAPI from './ClientToServerAPI';

/**
 * Atom effect that clears the atom's value when the current working directory / repository changes.
 */
export function clearOnCwdChange<T>(): AtomEffect<T> {
  return ({resetSelf}) => {
    serverAPI.onCwdChanged.on('change', resetSelf);
    return () => serverAPI.onCwdChanged.off('change', resetSelf);
  };
}
