/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atomFamilyWeak, atomLoadableWithRefresh} from '../jotaiUtils';
import type {DiffId} from '../types';
import serverAPI from '../ClientToServerAPI';

export const diffCommentData = atomFamilyWeak((diffId: DiffId) =>
  atomLoadableWithRefresh(async () => {
    serverAPI.postMessage({
      type: 'fetchDiffComments',
      diffId,
    });

    const result = await serverAPI.nextMessageMatching(
      'fetchedDiffComments',
      msg => msg.diffId === diffId,
    );
    if (result.comments.error != null) {
      throw new Error(result.comments.error.toString());
    }
    return result.comments.value;
  }),
);
