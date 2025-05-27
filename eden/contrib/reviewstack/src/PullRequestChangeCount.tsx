/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {gitHubPullRequest} from './recoil';
import {CounterLabel} from '@primer/react';
import {useRecoilValue} from 'recoil';

export default function PullRequestChangeCount(): React.ReactElement | null {
  const pullRequest = useRecoilValue(gitHubPullRequest);

  if (pullRequest == null) {
    return null;
  }

  const {additions, deletions} = pullRequest;

  return (
    <>
    <CounterLabel sx={{ backgroundColor: "success.muted" }}>+{additions}</CounterLabel>
    <CounterLabel scheme="primary" sx={{ backgroundColor: "danger.muted", color: "black" }}>-{deletions}</CounterLabel>
    </>
  );
}
