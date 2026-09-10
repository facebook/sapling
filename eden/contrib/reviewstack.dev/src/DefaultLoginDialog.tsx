/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 * Licensed under the MIT license in the upstream LICENSE file.
 */
import type {FormEvent} from 'react';
import type {CustomLoginDialogProps} from 'reviewstack/src/LoginDialog';

import './DefaultLoginDialog.css';
import {Box, Flash, Link, Text} from '@primer/react';
import {useEffect, useRef, useState} from 'react';

const GITHUB_AUTH_ENDPOINT = '/github/oauth2/auth';
const GITHUB_START_ENDPOINT = '/github/oauth2/start';
const GITHUB_RETURN_PARAMETER = 'reviewstack_github_oauth';

export default function DefaultLoginDialog({
  setTokenAndHostname,
  authError,
}: CustomLoginDialogProps) {
  const [token, setToken] = useState('');
  const [oauthError, setOAuthError] = useState<string | null>(null);
  const restoreOAuth = useRef(
    new URLSearchParams(window.location.search).get(GITHUB_RETURN_PARAMETER) === '1',
  ).current;
  const [checkingOAuth, setCheckingOAuth] = useState(restoreOAuth);
  const connectGitHub = useRef(setTokenAndHostname);
  connectGitHub.current = setTokenAndHostname;

  useEffect(() => {
    if (!restoreOAuth) {
      return;
    }
    let active = true;
    const returnURL = new URL(window.location.href);
    returnURL.searchParams.delete(GITHUB_RETURN_PARAMETER);
    window.history.replaceState(
      window.history.state,
      '',
      `${returnURL.pathname}${returnURL.search}${returnURL.hash}`,
    );
    async function restoreGitHubSession() {
      try {
        const response = await fetch(GITHUB_AUTH_ENDPOINT, {
          cache: 'no-store',
          credentials: 'same-origin',
        });
        if (!active) {
          return;
        }
        if (response.status === 401) {
          setCheckingOAuth(false);
          return;
        }
        if (response.status !== 202) {
          throw new Error(`GitHub sign-in returned HTTP ${response.status}`);
        }
        const oauthToken = response.headers.get('X-Auth-Request-Access-Token');
        if (oauthToken == null || oauthToken === '') {
          throw new Error('GitHub sign-in did not return an access token');
        }
        connectGitHub.current(oauthToken, 'github.com');
      } catch (error) {
        if (active) {
          setOAuthError(error instanceof Error ? error.message : 'GitHub sign-in failed');
          setCheckingOAuth(false);
        }
      }
    }
    restoreGitHubSession();
    return () => {
      active = false;
    };
  }, [restoreOAuth]);

  function startOAuth() {
    const returnURL = new URL(window.location.href);
    returnURL.searchParams.set(GITHUB_RETURN_PARAMETER, '1');
    const returnTo = `${returnURL.pathname}${returnURL.search}${returnURL.hash}`;
    window.location.assign(`${GITHUB_START_ENDPOINT}?rd=${encodeURIComponent(returnTo)}`);
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    if (token.trim()) {
      setTokenAndHostname(token.trim(), 'github.com');
      setToken('');
    }
  }
  return (
    <div className="LoginDialog-container">
      <Box className="LoginDialog" bg="canvas.default" borderWidth={1} borderColor="border.default">
        <Text as="h1" fontSize={3}>
          Aionic ReviewStack
        </Text>
        {authError && <Flash variant="warning">{authError}</Flash>}
        {oauthError && <Flash variant="warning">{oauthError}</Flash>}
        <Box as="p">Connect your GitHub account to read pull requests and submit reviews.</Box>
        <button type="button" onClick={startOAuth} disabled={checkingOAuth}>
          {checkingOAuth ? 'Checking GitHub sign-in…' : 'Authorize with GitHub'}
        </button>
        <Box as="p">
          GitHub shows the permissions before you approve them. Aionic controls the app and limits
          it to repositories where the app is installed.
        </Box>
        <details>
          <summary>Use a personal access token instead</summary>
          <form onSubmit={submit}>
            <Box as="p">
              Create a{' '}
              <Link
                href="https://github.com/settings/personal-access-tokens/new?target_name=aionic-labs&name=Aionic%20ReviewStack&contents=read&pull_requests=write"
                target="_blank"
                rel="noreferrer">
                fine-grained token
              </Link>{' '}
              for the repositories you review. Select the organization as the resource owner to
              access its private repositories.
            </Box>
            <Box as="p">
              Allow read access to Contents and read and write access to Pull requests.
            </Box>
            <Box as="p">
              Fine-grained tokens can limit CI check access. When checks are unavailable here, use
              the link to view them on GitHub.
            </Box>
            <Box as="p">
              <label htmlFor="github-token">GitHub token</label>
              <br />
              <input
                id="github-token"
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={token}
                required
                size={45}
                onChange={event => setToken(event.target.value)}
              />
            </Box>
            <button type="submit" disabled={!token.trim()}>
              Connect with token
            </button>
          </form>
        </details>
        <Box as="p">
          This browser stores your GitHub token and cached GitHub data. Use Logout to clear them.
        </Box>
      </Box>
    </div>
  );
}
