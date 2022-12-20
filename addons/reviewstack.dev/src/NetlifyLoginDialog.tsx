/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CustomLoginDialogProps} from 'reviewstack/src/LoginDialog';

import Footer from './Footer';
import {Box, Button, Heading, Text} from '@primer/react';
import Authenticator from 'netlify-auth-providers';
import {useCallback, useState} from 'react';
import AppHeader from 'reviewstack/src/AppHeader';
import Link from 'reviewstack/src/Link';

/**
 * See https://docs.github.com/en/developers/apps/building-oauth-apps/scopes-for-oauth-apps
 */
const GITHUB_OAUTH_SCOPE = ['user', 'repo'].join(' ');

export default function NetlifyLoginDialog({
  setTokenAndHostname,
}: CustomLoginDialogProps): React.ReactElement {
  const [isButtonDisabled, setButtonDisabled] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const onClick = useCallback(async () => {
    setButtonDisabled(true);
    try {
      const token = await fetchGitHubToken();
      setTokenAndHostname(token, 'github.com');
    } catch (e) {
      const message = e instanceof Error ? e.message : 'error fetching OAuth token';
      setErrorMessage(message);
    }
    setButtonDisabled(false);
  }, [setButtonDisabled, setErrorMessage, setTokenAndHostname]);

  return (
    <Box>
      <AppHeader orgAndRepo={null} />
      <Box margin="auto" width={800} paddingTop={2}>
        <Box pb={2}>
          <Heading>Welcome to ReviewStack</Heading>
          <Text as="p" pb={2}>
            <Link href="https://sapling-scm.com/docs/addons/reviewstack">ReviewStack</Link> is a
            novel user interface for GitHub pull requests with custom support for{' '}
            <Text fontStyle="italic">stacked changes</Text>. For tools like{' '}
            <Link href="http://sapling-scm.com/">Sapling</Link> or{' '}
            <Link href="https://github.com/ezyang/ghstack">ghstack</Link> that create separate pull
            requests for independent commits in a stack, ReviewStack facilitates navigating the
            stack and ensuring that only the code that was meant to be considered for review is
            displayed for each pull request.
          </Text>
          <Text as="p" pb={2}>
            ReviewStack is owned and operated by Meta Platforms, Inc. Note that{' '}
            <Link href="https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/authorizing-oauth-apps">
              you must authorize ReviewStack to access your GitHub data
            </Link>{' '}
            in order to use ReviewStack. Once authorized, ReviewStack will store your GitHub access
            token, and other data fetched from GitHub, locally in the browser. Clicking{' '}
            <Text fontWeight="bold">Logout</Text> will remove the data that is stored locally, but
            it will not delete your data from GitHub.
          </Text>
        </Box>
        {errorMessage != null ? (
          <Box pb={2}>
            <Text color="danger.fg">{errorMessage}</Text>
          </Box>
        ) : null}
        <Box>
          <Button onClick={onClick} disabled={isButtonDisabled}>
            Authorize ReviewStack to access GitHub
          </Button>
        </Box>
      </Box>
      <Footer />
    </Box>
  );
}

function fetchGitHubToken(): Promise<string> {
  return new Promise((resolve, reject) => {
    const authenticator = new Authenticator({});
    authenticator.authenticate(
      {provider: 'github', scope: GITHUB_OAUTH_SCOPE},
      (error: Error | null, data: {token: string} | null) => {
        if (error) {
          reject(error);
        } else {
          const token = data?.token;
          if (typeof token === 'string') {
            resolve(token);
          } else {
            reject(new Error('token missing in OAuth response'));
          }
        }
      },
    );
  });
}
