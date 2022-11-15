/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ServerPlatform} from '../src/serverPlatform';

export const platform: ServerPlatform = {
  handleMessageFromClient: async (_repo, message, _postMessage) => {
    switch (message.type) {
      // TODO: handle any android-studio platform file events
      case 'platform/openFile': {
        break;
      }
    }
  },
};
