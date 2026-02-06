/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  gitHubOrgAndRepoAtom,
  gitHubPullRequestIDAtom,
  pendingScrollRestoreAtom,
} from './jotai';
import {gitHubPullRequestForParams} from './recoil';
import {useAtomValue, useSetAtom} from 'jotai';
import {useCallback} from 'react';
import {useRecoilCallback} from 'recoil';

/**
 * @returns function that will mark the current PullRequest data to be refreshed
 *   with the latest data from the server. Rather than refresh
 *   `gitHubPullRequest` directly, this refreshes
 *   `gitHubPullRequestForParams(params)` so that `PullRequest.tsx` will derive
 *   a new value of `gitHubPullRequest` from the old one.
 *
 *   The refresh preserves the current scroll position to prevent the view
 *   from jumping when the data updates.
 */
export default function useRefreshPullRequest(): () => void {
  const number = useAtomValue(gitHubPullRequestIDAtom);
  const orgAndRepo = useAtomValue(gitHubOrgAndRepoAtom);
  const setPendingScrollRestore = useSetAtom(pendingScrollRestoreAtom);

  const recoilRefresh = useRecoilCallback(
    ({refresh}) =>
      () => {
        if (number == null || orgAndRepo == null) {
          return;
        }

        const params = {number, orgAndRepo};
        // Refreshing the selector here should trigger `PullRequest.tsx` to
        // update the gitHubPullRequestAtom.
        refresh(gitHubPullRequestForParams(params));
      },
    [number, orgAndRepo],
  );

  return useCallback(() => {
    // Save scroll position before refresh. This will be restored by
    // PullRequestWithParams after the pull request data updates.
    setPendingScrollRestore({
      scrollX: window.scrollX,
      scrollY: window.scrollY,
    });

    recoilRefresh();
  }, [recoilRefresh, setPendingScrollRestore]);
}

