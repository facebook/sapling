/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffId} from './types';

import {atom} from 'jotai';
import serverAPI from './ClientToServerAPI';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';

/**
 * In some cases, we need to explicitly disable message syncing after a failure.
 * This setting overrides the default value from the code review provider.
 * It's not intended to be set by users nor is it persisted across restarts.
 * When this is set, a warning will also be shown to the user.
 */
export const messageSyncingOverrideState = atom<boolean | null>(null);

/** Whether message syncing is enabled for the current repo. */
export const messageSyncingEnabledState = atom(get => {
  const override = get(messageSyncingOverrideState);
  if (override != null) {
    return override;
  }
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
