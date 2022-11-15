/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangeEvent, FormEvent} from 'react';
import type {CustomLoginDialogProps} from 'reviewstack/src/LoginDialog';

import './DefaultLoginDialog.css';

import {Box, Link, Text} from '@primer/react';
import {useCallback, useState} from 'react';

export default function LoginDialog({
  setToken: setTokenInLocalStorage,
}: CustomLoginDialogProps): React.ReactElement | null {
  const [token, setToken] = useState('');

  const onChangeToken = useCallback(
    (e: ChangeEvent) => setToken((e.target as HTMLInputElement).value),
    [],
  );

  const onSubmit = useCallback(
    (e: FormEvent) => {
      e.preventDefault();
      setTokenInLocalStorage(token.trim());
      return false;
    },
    [token, setTokenInLocalStorage],
  );

  const isInputValid = isValid(token);

  return (
    <>
      <Box className="LoginDialog-overlay" bg="canvas.subtle" />
      <div className="LoginDialog-container">
        <Box
          bg="canvas.default"
          className="LoginDialog"
          borderWidth={1}
          borderColor="border.default">
          <form onSubmit={onSubmit}>
            <div className="LoginDialog-field">
              <Text>
                This tool requires an authentication token so it can read and write data from
                GitHub. Follow GitHub's{' '}
                <Link
                  href="https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/creating-a-personal-access-token"
                  target="_blank">
                  instructions to create a personal access token (PAT)
                </Link>{' '}
                and <Text fontWeight="bold">be sure to store it in a safe place</Text>. After
                initially viewing your PAT, GitHub will never show it to you again.
              </Text>
              {/* We may add a checkbox to aide in persisting these values. */}
            </div>
            <Box paddingY={2}>
              <Text fontStyle="italic">
                Note your PAT will be stored in <code>localStorage</code> so you will not have to
                enter it again when you return to this page. Click{' '}
                <Text fontStyle="normal">Logout</Text> to delete your PAT and any data that was
                fetched from GitHub using your PAT from the browser.
              </Text>
            </Box>
            <Box pb={2}>
              Personal Access Token: <br />
              <input
                value={token}
                size={60}
                required={true}
                onChange={onChangeToken}
                placeholder="paste your token here"
              />
            </Box>
            <div>
              <input
                type="submit"
                value="Grant access to your GitHub data"
                disabled={!isInputValid}
              />
            </div>
          </form>
        </Box>
      </div>
    </>
  );
}

function isValid(token: string): boolean {
  return token.trim() !== '';
}
