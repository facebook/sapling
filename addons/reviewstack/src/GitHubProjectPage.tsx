/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import Link from './Link';
import URLFor from './URLFor';
import {gitHubHostname} from './github/gitHubCredentials';
import {MarkGithubIcon} from '@primer/octicons-react';
import {Box, Text} from '@primer/react';
import {useRecoilValue} from 'recoil';

export default function GitHubProjectPage(props: {org: string; repo: string}): React.ReactElement {
  const orgRepo = `${props.org}/${props.repo}`;
  const hostname = useRecoilValue(gitHubHostname);
  return (
    <Box padding={2}>
      <Box pb={2}>
        <Link href={`https://${hostname}${URLFor.project(props)}`}>
          <Text>
            View {orgRepo} on GitHub <MarkGithubIcon />
          </Text>
        </Link>
      </Box>
      <Box>
        <Link href={URLFor.pulls(props)}>
          <Text>View pull requests for {orgRepo}</Text>
        </Link>
      </Box>
    </Box>
  );
}
