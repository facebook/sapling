/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffId, PullRequestReviewAction, PullRequestReviewData} from '../types';

import {randomId} from 'shared/utils';
import serverAPI from '../ClientToServerAPI';
import {atomFamilyWeak, atomLoadableWithRefresh} from '../jotaiUtils';

export const pullRequestReviewData = atomFamilyWeak((diffId: DiffId) =>
  atomLoadableWithRefresh(async () => {
    if (diffId === '') {
      return undefined;
    }
    serverAPI.postMessage({type: 'fetchPullRequestReview', diffId});
    const result = await serverAPI.nextMessageMatching(
      'fetchedPullRequestReview',
      message => message.diffId === diffId,
    );
    if (result.review.error != null) {
      throw result.review.error;
    }
    return result.review.value;
  }),
);

export async function runPullRequestReviewAction(
  diffId: DiffId,
  action: PullRequestReviewAction,
): Promise<PullRequestReviewData> {
  const requestId = randomId();
  serverAPI.postMessage({type: 'runPullRequestReviewAction', diffId, requestId, action});
  const result = await serverAPI.nextMessageMatching(
    'pullRequestReviewActionResult',
    message => message.requestId === requestId,
  );
  if (result.review.error != null) {
    throw result.review.error;
  }
  return result.review.value;
}
