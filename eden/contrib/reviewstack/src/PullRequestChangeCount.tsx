/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {gitHubPullRequestComparisonFilesAtom} from './jotai';
import sumFileLineChanges from './sumFileLineChanges';
import {CounterLabel} from '@primer/react';
import {useAtomValue} from 'jotai';

export default function PullRequestChangeCount(): React.ReactElement | null {
  const comparisonFiles = useAtomValue(gitHubPullRequestComparisonFilesAtom);
  const {additions, deletions} = sumFileLineChanges(comparisonFiles);

  return (
    <>
      <CounterLabel sx={{backgroundColor: 'success.muted'}}>+{additions}</CounterLabel>
      <CounterLabel scheme="primary" sx={{backgroundColor: 'danger.muted', color: 'black'}}>
        -{deletions}
      </CounterLabel>
    </>
  );
}
