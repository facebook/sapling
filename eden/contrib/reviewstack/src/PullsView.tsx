/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import CenteredSpinner from './CenteredSpinner';
import Pulls from './Pulls';
import PullsHeader from './PullsHeader';
import {APP_HEADER_HEIGHT} from './constants';
import {gitHubOrgAndRepo} from './recoil';
import {Box} from '@primer/react';
import {Suspense, useEffect} from 'react';
import {useSetRecoilState} from 'recoil';

const HEADER_HEIGHT = 60;
const TOTAL_HEADER_HEIGHT = HEADER_HEIGHT + APP_HEADER_HEIGHT;

type Props = {
  org: string;
  repo: string;
};

export default function PullsView({org, repo}: Props): React.ReactElement {
  const setOrgAndRepo = useSetRecoilState(gitHubOrgAndRepo);

  useEffect(() => {
    setOrgAndRepo({org, repo});
  }, [org, repo, setOrgAndRepo]);

  return (
    <Suspense fallback={<CenteredSpinner message="Loading pull requests..." />}>
      <PullsHeader height={HEADER_HEIGHT} />
      <Box height={`calc(100vh - ${TOTAL_HEADER_HEIGHT}px)`} overflow="auto">
        <Pulls />
      </Box>
    </Suspense>
  );
}
