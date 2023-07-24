/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {selector} from 'recoil';

export const messageSyncingEnabledState = selector({
  key: 'messageSyncingEnabledState',
  get: ({get}) => {
    const provider = get(codeReviewProvider);
    return provider?.enableMessageSyncing ?? false;
  },
});
