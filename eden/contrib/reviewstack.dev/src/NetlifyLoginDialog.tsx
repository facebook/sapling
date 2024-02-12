/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangeEvent, FormEvent} from 'react';
import type {CustomLoginDialogProps} from 'reviewstack/src/LoginDialog';

import Footer from './Footer';
import InlineCode from './InlineCode';
import {Box, Button, Heading, Text, TextInput} from '@primer/react';
import Authenticator from 'netlify-auth-providers';
import React, {useCallback, useState} from 'react';
import AppHeader from 'reviewstack/src/AppHeader';
import Link from 'reviewstack/src/Link';

/**
 * See https://docs.github.com/en/developers/apps/building-oauth-apps/scopes-for-oauth-apps
 */
const GITHUB_OAUTH_SCOPE = ['user', 'repo'].join(' ');

export default function NetlifyLoginDialog(props: CustomLoginDialogProps): React.ReactElement {
  return (
    <Box display="flex" flexDirection="column" height="100vh">
      <Box flex="0 0 auto">
        <AppHeader orgAndRepo={null} />
      </Box>
      <Box flex="1 1 auto" overflowY="auto">
        <Box
          display="flex"
          flexDirection="row"
          justifyContent="space-between"
          paddingX={3}
          paddingY={2}>
          <Box minWidth={600} maxWidth={800}>
            <EndUserInstructions {...props} />
          </Box>
          <Box>
            <img src="/reviewstack-demo.gif" width={800} />
            <Box textAlign="center" width={800}>
              <Text fontStyle="italic" fontSize={1}>
                ReviewStack makes it possible to view code and the timeline side-by-side
                <br />
                in addition to facilitating navigation up and down the stack.
              </Text>
            </Box>
          </Box>
        </Box>
      </Box>
      <Footer />
    </Box>
  );
}

function EndUserInstructions(props: CustomLoginDialogProps): React.ReactElement {
  const {setTokenAndHostname} = props;
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
      <Heading>Welcome to ReviewStack</Heading>
      <Box>
        <Text as="p" pb={2}>
          <Link href="https://sapling-scm.com/docs/addons/reviewstack">ReviewStack</Link> is a novel
          user interface for GitHub pull requests with custom support for{' '}
          <Text fontStyle="italic">stacked changes</Text>. For tools like{' '}
          <Link href="http://sapling-scm.com/">Sapling</Link> or{' '}
          <Link href="https://github.com/ezyang/ghstack">ghstack</Link> that create separate pull
          requests for independent commits in a stack, ReviewStack facilitates navigating the stack
          and ensuring that only the code that was meant to be considered for review is displayed
          for each pull request.
        </Text>
        <Text as="p">
          ReviewStack (reviewstack.dev) is owned and operated by Meta Platforms, Inc. Note that{' '}
          <Link href="https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/authorizing-oauth-apps">
            you must authorize ReviewStack to access your GitHub data
          </Link>{' '}
          in order to use ReviewStack. Once authorized, ReviewStack will store your GitHub access
          token, and other data fetched from GitHub, locally in the browser. Clicking{' '}
          <Text fontWeight="bold">Logout</Text> will remove the data that is stored locally, but it
          will not delete your data from GitHub.
        </Text>
      </Box>
      {errorMessage != null ? (
        <Box pb={2}>
          <Text color="danger.fg">{errorMessage}</Text>
        </Box>
      ) : null}

      <H3>github.com Users</H3>
      <Box pb={4}>
        To view code hosted on github.com using ReviewStack, you can use the OAuth flow below to
        authenticate with GitHub:
      </Box>
      <Box pb={4}>
        <Button onClick={onClick} disabled={isButtonDisabled}>
          Use OAuth to Authorize ReviewStack to access GitHub
        </Button>
      </Box>

      <H3>GitHub Enterprise Users</H3>
      <Box pb={4}>
        To use ReviewStack to view code on your <Text fontWeight="bold">GitHub Enterprise</Text>{' '}
        account, you must specify the hostname along with a{' '}
        <Link href="https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/creating-a-personal-access-token">
          Personal Access Token (PAT)
        </Link>{' '}
        for the account. If you have authenticated with the{' '}
        <Link href="https://cli.github.com/">GitHub CLI</Link>, you can use{' '}
        <InlineCode>gh auth status --show-token --hostname HOSTNAME</InlineCode> to use your
        existing token, though note the PAT for <InlineCode>gh</InlineCode> may have access to a
        broader set of{' '}
        <Link href="https://docs.github.com/en/developers/apps/building-oauth-apps/scopes-for-oauth-apps">
          scopes
        </Link>{' '}
        than you may be willing to grant to a third-party tool.
      </Box>
      <EnterpriseForm {...props} />

      <H3>Building From Source</H3>
      <Box>
        Finally, if you want to run your own instance of ReviewStack,{' '}
        <Link href="https://github.com/facebook/sapling/tree/main/addons/reviewstack">
          the source code is available on GitHub
        </Link>
        . For example, you may want to deploy your own instance of ReviewStack that is hardcoded to
        work with your GitHub Enterprise instance combined with an in-house SSO login flow. The
        codebase is designed so that the ReviewStack UI can be used as a reusable React component
        with minimal dependencies on the host environment.
      </Box>
    </Box>
  );
}

function EnterpriseForm({setTokenAndHostname}: CustomLoginDialogProps): React.ReactElement {
  const [hostname, setHostname] = useState('github.com');
  const [token, setToken] = useState('');

  const onChangeHostname = useCallback(
    (e: ChangeEvent) => setHostname((e.target as HTMLInputElement).value),
    [],
  );
  const onChangeToken = useCallback(
    (e: ChangeEvent) => setToken((e.target as HTMLInputElement).value),
    [],
  );
  const onSubmit = useCallback(
    (e: FormEvent) => {
      e.preventDefault();
      setTokenAndHostname(token.trim(), hostname.trim());
      return false;
    },
    [token, hostname, setTokenAndHostname],
  );

  // FIXME: mention `gh auth status`
  const isInputValid = isGitHubEnterpriseInputValid(token, hostname);
  return (
    <Box>
      <form onSubmit={onSubmit}>
        <Box pb={2}>
          GitHub Enterprise Hostname: <br />
          <TextInput
            value={hostname}
            onChange={onChangeHostname}
            sx={{width: '400px'}}
            monospace
            aria-label="hostname"
            placeholder="github.com"
          />
        </Box>
        <Box>
          Personal Access Token: <br />
          <TextInput
            value={token}
            onChange={onChangeToken}
            type="password"
            sx={{width: '400px'}}
            monospace
            aria-label="personal access token"
            placeholder="github_pat_abcdefg123456789"
          />
        </Box>
        <Box paddingY={4}>
          <Button disabled={!isInputValid} type="submit">
            Use your GitHub Enterprise account with ReviewStack
          </Button>
        </Box>
      </form>
    </Box>
  );
}

function isGitHubEnterpriseInputValid(token: string, hostname: string): boolean {
  if (token.trim() === '') {
    return false;
  }

  const normalizedHostname = hostname.trim();
  return normalizedHostname !== '' && normalizedHostname.indexOf('.') !== -1;
}

function H3({children}: {children: React.ReactNode}): React.ReactElement {
  return (
    <Heading as="h3" sx={{fontSize: 3, mb: 2}}>
      {children}
    </Heading>
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
