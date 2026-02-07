/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitHubTokenState} from './jotai';

import {gitHubTokenPersistenceAtom, gitHubTokenStateAtom, gitHubUsernameAtom} from './jotai';
import {Link, Text} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {loadable} from 'jotai/utils';
import {useCallback, useMemo} from 'react';

/**
 * Get the token value from the token state.
 */
function getTokenValue(state: GitHubTokenState): string | null {
  if (state.state === 'hasValue') {
    return state.value;
  }
  return null;
}

export default function Username(): React.ReactElement | null {
  // Get username via loadable to handle async state
  const loadableUsernameAtom = useMemo(() => loadable(gitHubUsernameAtom), []);
  const usernameLoadable = useAtomValue(loadableUsernameAtom);
  const username = usernameLoadable.state === 'hasData' ? usernameLoadable.data : null;

  // Get token state directly for checking current value
  const tokenState = useAtomValue(gitHubTokenStateAtom);
  const token = getTokenValue(tokenState);

  const setToken = useSetAtom(gitHubTokenPersistenceAtom);
  const onLogout = useCallback(() => setToken(null), [setToken]);

  // Show UI when we have a token (regardless of loading state)
  if (tokenState.state === 'hasValue' && token != null) {
    if (username != null) {
      return (
        <>
          <Text fontWeight="bold">{username}</Text>
          {' | '}
          <Link as="button" onClick={onLogout}>
            Logout
          </Link>
        </>
      );
    } else {
      // we have a token but no username: we still offer the logout button
      return (
        <>
          <Link as="button" onClick={onLogout}>
            Logout
          </Link>
        </>
      );
    }
  }

  return null;
}
