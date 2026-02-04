/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {gitHubPullRequestAtom} from './jotai';
import {CounterLabel} from '@primer/react';
import {useAtomValue} from 'jotai';

export default function PullRequestChangeCount(): React.ReactElement | null {
  const pullRequest = useAtomValue(gitHubPullRequestAtom);

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
