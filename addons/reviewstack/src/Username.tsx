/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {gitHubTokenPersistence, gitHubUsername} from './github/gitHubCredentials';
import {Link, Text} from '@primer/react';
import {useCallback} from 'react';
import {useRecoilStateLoadable, useRecoilValueLoadable} from 'recoil';

export default function Username(): React.ReactElement | null {
  const username = useRecoilValueLoadable(gitHubUsername).valueMaybe();
  const [tokenPersistenceLoadable, setToken] = useRecoilStateLoadable(gitHubTokenPersistence);
  const onLogout = useCallback(() => setToken(null), [setToken]);

  switch (tokenPersistenceLoadable.state) {
    case 'hasValue': {
      const {contents: token} = tokenPersistenceLoadable;
      if (username != null && token != null) {
        return (
          <>
            <Text fontWeight="bold">{username}</Text>
            {' | '}
            <Link as="button" onClick={onLogout}>
              Logout
            </Link>
          </>
        );
      } else if (token != null) {
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
  }

  return null;
}
