/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  gitHubOrgAndRepoAtom,
  gitHubPullRequestIDAtom,
  gitHubPullRequestRefreshTriggerAtom,
  pendingScrollRestoreAtom,
} from './jotai';
import {useAtomValue, useSetAtom, useStore} from 'jotai';
import {useCallback} from 'react';

/**
 * @returns function that will mark the current PullRequest data to be refreshed
 *   with the latest data from the server. This increments the refresh trigger
 *   atom which causes the gitHubPullRequestForParamsAtom to re-evaluate.
 *
 *   The refresh preserves the current scroll position to prevent the view
 *   from jumping when the data updates.
 */
export default function useRefreshPullRequest(): () => void {
  const number = useAtomValue(gitHubPullRequestIDAtom);
  const orgAndRepo = useAtomValue(gitHubOrgAndRepoAtom);
  const setPendingScrollRestore = useSetAtom(pendingScrollRestoreAtom);
  const store = useStore();

  return useCallback(() => {
    if (number == null || orgAndRepo == null) {
      return;
    }

    // Save scroll position before refresh. This will be restored by
    // PullRequestWithParams after the pull request data updates.
    setPendingScrollRestore({
      scrollX: window.scrollX,
      scrollY: window.scrollY,
    });

    const params = {number, orgAndRepo};
    // Increment the refresh trigger to cause the PR atom to re-fetch
    const refreshTriggerAtom = gitHubPullRequestRefreshTriggerAtom(params);
    const currentValue = store.get(refreshTriggerAtom);
    store.set(refreshTriggerAtom, currentValue + 1);
  }, [number, orgAndRepo, setPendingScrollRestore, store]);
}
