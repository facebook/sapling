/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitObjectID, VersionCommit} from './github/types';

import PullRequestVersionCommitSelectorItem from './PullRequestVersionCommitSelectorItem';
import {
  gitHubPullRequestComparableVersions,
  gitHubPullRequestSelectedVersionCommits,
} from './recoil';
import {shortOid} from './utils';
import {ActionList, ActionMenu} from '@primer/react';
import React from 'react';
import {useRecoilValue} from 'recoil';
import {notEmpty} from 'shared/utils';

type Props = {
  org: string;
  repo: string;
};

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PullRequestVersionCommitSelector({
  org,
  repo,
}: Props): React.ReactElement {
  const {beforeCommitID, afterCommitID} = useRecoilValue(gitHubPullRequestComparableVersions);
  const commits = useRecoilValue(gitHubPullRequestSelectedVersionCommits);
  const beforeIndex = getIndex(beforeCommitID, commits, -1);
  const afterIndex = getIndex(afterCommitID, commits, commits.length - 1);

  return (
    <ActionMenu>
      <ActionMenu.Button>{commitsLabel(commits, beforeIndex, afterIndex)}</ActionMenu.Button>
      <ActionMenu.Overlay width="xxlarge">
        <ActionList>
          {commits.map(({author, commit, committedDate, title}, index) => (
            <PullRequestVersionCommitSelectorItem
              key={index}
              beforeCommitID={beforeCommitID}
              afterCommitID={afterCommitID}
              author={author}
              commit={commit}
              commits={commits}
              committedDate={committedDate}
              index={index}
              maxIndex={commits.length - 1}
              message={title}
              beforeIndex={beforeIndex}
              afterIndex={afterIndex}
              org={org}
              repo={repo}
            />
          ))}
        </ActionList>
      </ActionMenu.Overlay>
    </ActionMenu>
  );
});

function getIndex(
  commitID: GitObjectID | null,
  commits: VersionCommit[],
  defaultValue: number,
): number {
  const index = commitID === null ? -1 : commits.findIndex(({commit}) => commit === commitID);
  return index === -1 ? defaultValue : index;
}

function commitsLabel(commits: VersionCommit[], beforeIndex: number, afterIndex: number): string {
  return [commits[beforeIndex]?.commit, commits[afterIndex]?.commit]
    .filter(notEmpty)
    .map(shortOid)
    .join(' vs. ');
}
