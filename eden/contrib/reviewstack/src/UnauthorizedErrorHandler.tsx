/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import CenteredSpinner from './CenteredSpinner';
import {authErrorMessageAtom, gitHubTokenPersistenceAtom} from './jotai';
import {useEffect} from 'react';
import {useSetAtom} from 'jotai';

type Props = {
  message: string;
};

/**
 * Component that handles unauthorized (401) errors by clearing the token
 * and setting an error message to display on the login page.
 */
export default function UnauthorizedErrorHandler({message}: Props): React.ReactElement {
  const setToken = useSetAtom(gitHubTokenPersistenceAtom);
  const setAuthError = useSetAtom(authErrorMessageAtom);

  useEffect(() => {
    // Set the error message first so it's available when the login dialog shows
    setAuthError(message);
    // Clear the token, which will cause the app to show the login dialog
    setToken(null);
  }, [message, setToken, setAuthError]);

  // Show a loading spinner while the token is being cleared
  return <CenteredSpinner message="Session expired. Redirecting to login..." />;
}
