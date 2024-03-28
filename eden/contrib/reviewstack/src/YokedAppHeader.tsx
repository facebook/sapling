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
import {YOKE_APP_HEADER_HEIGHT} from './constants';
import {primerColorMode} from './themeState';
import {HomeIcon} from '@primer/octicons-react';
import {Box, Header, Text, ToggleSwitch} from '@primer/react';
import {useCallback} from 'react';
import {useRecoilState} from 'recoil';

import YokedLogo from './YokedLogo';

type Props = {
  orgAndRepo: GitHubOrgAndRepo | null;
};

export default function AppHeader({orgAndRepo}: Props): React.ReactElement {
  return (
    <div className="header">
      <div className="nav">
        <div className="nav-items">
          <div className="nav-item nav-brand">
            <YokedLogo />
          </div>
          <div className="nav-item">
            <Username />
          </div>
        </div>
      </div>
    </div>
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
