/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {gitHubPullRequest, pullRequestSLOC} from './recoil';
import {CounterLabel, Tooltip} from '@primer/react';
import {useRecoilValue} from 'recoil';

export default function PullRequestChangeCount(): React.ReactElement | null {
  const pullRequest = useRecoilValue(gitHubPullRequest);
  const slocInfo = useRecoilValue(pullRequestSLOC);

  if (pullRequest == null) {
    return null;
  }

  const {additions, deletions} = pullRequest;
  const {significantLines, generatedFileCount} = slocInfo;

  const tooltipText =
    generatedFileCount > 0
      ? `${significantLines} significant lines (excludes ${generatedFileCount} generated file${generatedFileCount === 1 ? '' : 's'})`
      : `${significantLines} significant lines`;

  return (
    <>
      <CounterLabel sx={{backgroundColor: 'success.muted'}}>+{additions}</CounterLabel>
      <CounterLabel
        scheme="primary"
        sx={{backgroundColor: 'danger.muted', color: 'black'}}>
        -{deletions}
      </CounterLabel>
      {significantLines > 0 && (
        <Tooltip aria-label={tooltipText} direction="s">
          <CounterLabel
            sx={{
              backgroundColor: 'accent.subtle',
              color: 'fg.default',
              marginLeft: 1,
            }}>
            {significantLines} sig
          </CounterLabel>
        </Tooltip>
      )}
    </>
  );
}
