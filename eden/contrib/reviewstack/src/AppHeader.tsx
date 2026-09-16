/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitHubOrgAndRepo} from './jotai';

import Link from './Link';
import URLFor from './URLFor';
import Username from './Username';
import {APP_HEADER_HEIGHT} from './constants';
import {primerColorModeAtom} from './jotai/atoms';
import {MoonIcon, StackIcon, SunIcon} from '@primer/octicons-react';
import {Box, Header} from '@primer/react';
import {useAtom} from 'jotai';
import {useCallback} from 'react';

import './AppHeader.css';

type Props = {
  orgAndRepo: GitHubOrgAndRepo | null;
};

export default function AppHeader({orgAndRepo}: Props): React.ReactElement {
  return (
    <Header
      className="reviewstack-app-header"
      sx={{
        fontSize: 2,
        height: APP_HEADER_HEIGHT,
        justifyContent: 'space-between',
      }}>
      <Header.Item className="reviewstack-header-left">
        <Link href="/">
          <span className="reviewstack-brand">
            <StackIcon size={24} aria-hidden="true" />
            <span>ReviewStack</span>
          </span>
        </Link>
        <nav className="reviewstack-primary-nav" aria-label="Primary navigation">
          <Link href="/">
            <span className="reviewstack-nav-item">Review work</span>
          </Link>
          {orgAndRepo != null && <ProjectLink {...orgAndRepo} />}
        </nav>
      </Header.Item>
      <Header.Item className="reviewstack-header-actions">
        <ThemeSelector />
        <Box className="reviewstack-header-user">
          <Username />
        </Box>
      </Header.Item>
    </Header>
  );
}

function ProjectLink({org, repo}: {org: string; repo: string}): React.ReactElement {
  return (
    <Link href={URLFor.project({org, repo})}>
      <span className="reviewstack-nav-item reviewstack-project-link">
        {org} / {repo}
      </span>
    </Link>
  );
}

function ThemeSelector(): React.ReactElement {
  const [colorMode, setColorMode] = useAtom(primerColorModeAtom);
  const isDark = colorMode === 'night';
  const onClick = useCallback(() => {
    setColorMode(isDark ? 'day' : 'night');
  }, [isDark, setColorMode]);
  return (
    <button
      className="reviewstack-theme-button"
      type="button"
      onClick={onClick}
      aria-label={isDark ? 'Switch to light mode' : 'Switch to dark mode'}>
      {isDark ? <SunIcon size={18} /> : <MoonIcon size={18} />}
    </button>
  );
}
