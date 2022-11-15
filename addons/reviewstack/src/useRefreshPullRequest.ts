/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {gitHubPullRequestID, gitHubOrgAndRepo, gitHubPullRequestForParams} from './recoil';
import {useRecoilCallback} from 'recoil';

/**
 * @returns function that will mark the current PullRequest data to be refreshed
 *   with the latest data from the server. Rather than refresh
 *   `gitHubPullRequest` directly, this refreshes
 *   `gitHubPullRequestForParams(params)` so that `PullRequest.tsx` will derive
 *   a new value of `gitHubPullRequest` from the old one.
 */
export default function useRefreshPullRequest(): () => void {
  return useRecoilCallback(
    ({snapshot, refresh}) =>
      () => {
        const number = snapshot.getLoadable(gitHubPullRequestID).valueMaybe();
        const orgAndRepo = snapshot.getLoadable(gitHubOrgAndRepo).valueMaybe();
        if (number == null || orgAndRepo == null) {
          return;
        }

        const params = {number, orgAndRepo};
        // Refreshing the selector here should trigger `PullRequest.tsx` to
        // update the gitHubPullRequest atom.
        refresh(gitHubPullRequestForParams(params));
      },
    [],
  );
}
