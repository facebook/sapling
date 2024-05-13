/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import serverAPI from './ClientToServerAPI';
import {lazyAtom} from './jotaiUtils';

export const externalMergeToolAtom = lazyAtom(() => {
  serverAPI.onConnectOrReconnect(() => {
    serverAPI.postMessage({
      type: 'getConfiguredMergeTool',
    });
  });
  return serverAPI
    .nextMessageMatching('gotConfiguredMergeTool', () => true)
    .then(event => event.tool);
}, undefined);
