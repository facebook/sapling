/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitHubOrgAndRepo} from './recoil';

import Link from './Link';
import URLFor from './URLFor';
import Username from './Username';
import {APP_HEADER_HEIGHT} from './constants';
import {primerColorMode} from './themeState';
import {HomeIcon} from '@primer/octicons-react';
import {Box, Header, Text, ToggleSwitch} from '@primer/react';
import {useCallback} from 'react';
import {useRecoilState} from 'recoil';

type Props = {
  orgAndRepo: GitHubOrgAndRepo | null;
};

export default function AppHeader({orgAndRepo}: Props): React.ReactElement {
  return (
    <Header
      sx={{
        fontSize: 2,
        height: APP_HEADER_HEIGHT,
        justifyContent: 'space-between',
      }}>
      <Header.Item>
        <Box pr={2}>
          <Link href="/">
            <HomeIcon size="medium" aria-label="homepage" />
          </Link>
        </Box>
        <Box>{orgAndRepo != null && <PullsLink {...orgAndRepo} />}</Box>
      </Header.Item>
      <Header.Item>
        <Box>
          <ThemeSelector />
          <Username />
        </Box>
      </Header.Item>
    </Header>
  );
}

function PullsLink({org, repo}: {org: string; repo: string}) {
  return (
    <Link href={URLFor.project({org, repo})}>
      <Text color="fg.onEmphasis" fontWeight="bold">
        {org}
        {' / '}
        {repo}
      </Text>
    </Link>
  );
}

function ThemeSelector() {
  const [colorMode, setColorMode] = useRecoilState(primerColorMode);
  const checked = colorMode === 'night';
  const onClick = useCallback(() => {
    setColorMode(colorMode === 'night' ? 'day' : 'night');
  }, [colorMode, setColorMode]);
  // sx trick to hide label taken from https://github.com/primer/react/issues/2078
  const sx = {'> [aria-hidden]': {display: 'none'}};
  return (
    <Text>
      <span id="theme-switch-label">Dark Mode</span>:{' '}
      <ToggleSwitch
        checked={checked}
        onClick={onClick}
        size="small"
        aria-labelledby="theme-switch-label"
        sx={sx}
      />{' '}
    </Text>
  );
}
