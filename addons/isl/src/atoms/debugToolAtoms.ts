/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom} from 'jotai';
import serverAPI from '../ClientToServerAPI';
import {atomWithOnChange, localStorageBackedAtom} from '../jotaiUtils';

export const enableReduxTools = localStorageBackedAtom<boolean>('isl.debug-redux-tools', false);

export const enableReactTools = localStorageBackedAtom<boolean>('isl.debug-react-tools', false);

export const enableSaplingDebugFlag = atomWithOnChange<boolean>(
  atom(false),
  enabled => {
    serverAPI.postMessage({type: 'setDebugLogging', name: 'debug', enabled});
  },
  /* skipInitialCall */ true,
);

export const enableSaplingVerboseFlag = atomWithOnChange<boolean>(
  atom(false),
  enabled => {
    serverAPI.postMessage({type: 'setDebugLogging', name: 'verbose', enabled});
  },
  /* skipInitialCall */ true,
);
