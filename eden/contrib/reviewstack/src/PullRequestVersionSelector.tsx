/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import PullRequestVersionSelectorItem from './PullRequestVersionSelectorItem';
import {gitHubPullRequestSelectedVersionIndexAtom, gitHubPullRequestVersionsAtom} from './jotai';
import {versionLabel} from './utils';
import {ActionList, ActionMenu} from '@primer/react';
import {useAtomValue} from 'jotai';
import React from 'react';

type Props = {
  org: string;
  repo: string;
};

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PullRequestVersionSelector({
  org,
  repo,
}: Props): React.ReactElement {
  const versions = useAtomValue(gitHubPullRequestVersionsAtom);
  const selectedVersionIndex = useAtomValue(gitHubPullRequestSelectedVersionIndexAtom);

  return (
    <ActionMenu>
      <ActionMenu.Button>{versionLabel(selectedVersionIndex)}</ActionMenu.Button>
      <ActionMenu.Overlay width="large">
        <ActionList selectionVariant="single">
          {versions.map(({headCommit, headCommittedDate, baseParent, commits}, index) => (
            <PullRequestVersionSelectorItem
              key={index}
              baseParent={baseParent}
              commit={headCommit}
              commits={commits}
              committedDate={headCommittedDate}
              index={index}
              org={org}
              repo={repo}
            />
          ))}
        </ActionList>
      </ActionMenu.Overlay>
    </ActionMenu>
  );
});
