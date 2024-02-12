/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffId} from './types';

import serverAPI from './ClientToServerAPI';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {atom} from 'jotai';

export const messageSyncingEnabledState = atom(get => {
  const provider = get(codeReviewProvider);
  return provider?.enableMessageSyncing ?? false;
});

export async function updateRemoteMessage(
  diffId: DiffId,
  title: string,
  description: string,
): Promise<void> {
  serverAPI.postMessage({type: 'updateRemoteDiffMessage', diffId, title, description});
  const response = await serverAPI.nextMessageMatching(
    'updatedRemoteDiffMessage',
    msg => msg.diffId === diffId,
  );
  if (response.error != null) {
    throw new Error(response.error);
  }
}
