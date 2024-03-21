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
            {/* <HomeIcon size="medium" aria-label="homepage" /> */}
            <svg
              xmlns="http://www.w3.org/2000/svg"
              viewBox="0 0 24 19.2"
              style={{width: '24px', height: '24px'}}>
              <path
                className="cls-1"
                style={{fill: 'white', strokeWidth: 0}}
                d="M20.18,11.53c.4.29.89.47,1.42.47,1.33,0,2.4-1.07,2.4-2.4s-1.07-2.4-2.4-2.4-2.4,1.07-2.4,2.4c0,.36.08.7.22,1l-5.62,4.58,7.01-10.52c.25.09.51.13.79.13,1.33,0,2.4-1.07,2.4-2.4s-1.07-2.4-2.4-2.4-2.4,1.07-2.4,2.4c0,.61.23,1.18.61,1.6l-7.02,10.53c-.25-.09-.51-.13-.79-.13s-.54.05-.79.13L4.19,4c.38-.42.61-.99.61-1.6,0-1.33-1.07-2.4-2.4-2.4S0,1.07,0,2.4s1.07,2.4,2.4,2.4c.28,0,.54-.05.79-.13l7.01,10.52-5.62-4.58c.14-.31.22-.64.22-1,0-1.33-1.07-2.4-2.4-2.4s-2.4,1.07-2.4,2.4,1.07,2.4,2.4,2.4c.53,0,1.02-.17,1.42-.47l5.73,4.67h-4.82c-.27-1.04-1.21-1.8-2.32-1.8-1.33,0-2.4,1.07-2.4,2.4s1.07,2.4,2.4,2.4c1.12,0,2.06-.76,2.32-1.8h4.95c.27,1.04,1.21,1.8,2.32,1.8s2.06-.76,2.32-1.8h4.95c.27,1.04,1.21,1.8,2.32,1.8,1.33,0,2.4-1.07,2.4-2.4s-1.07-2.4-2.4-2.4c-1.12,0-2.06.76-2.32,1.8h-4.82l5.73-4.67Z"
              />
            </svg>
          </Link>
        </Box>
        Modelcode / Reputation
        {/* <Box>{orgAndRepo != null && <PullsLink {...orgAndRepo} />}</Box> */}
      </Header.Item>
      {/* <Header.Item>
        <Box>
          <ThemeSelector />
          <Username />
        </Box>
      </Header.Item> */}
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
