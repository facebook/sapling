/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {authErrorMessageAtom, gitHubHostnameAtom, gitHubTokenPersistenceAtom} from './jotai';
import {useAtomValue, useSetAtom} from 'jotai';

export type CustomLoginDialogProps = {
  setTokenAndHostname(token: string, hostname: string): void;
  /** Error message to display, typically when the previous token was invalid */
  authError: string | null;
};

let CustomLoginDialogComponent: React.FunctionComponent<CustomLoginDialogProps> | null = null;

export function setCustomLoginDialogComponent(
  component: React.FunctionComponent<CustomLoginDialogProps>,
) {
  CustomLoginDialogComponent = component;
}

export default function LoginDialog(): React.ReactElement {
  const setToken = useSetAtom(gitHubTokenPersistenceAtom);
  const setHostname = useSetAtom(gitHubHostnameAtom);
  const authError = useAtomValue(authErrorMessageAtom);
  const setAuthError = useSetAtom(authErrorMessageAtom);

  function setTokenAndHostname(token: string, hostname: string): void {
    // Clear any previous auth error when setting a new token
    setAuthError(null);
    setHostname(hostname);
    setToken(token);
  }
  if (CustomLoginDialogComponent != null) {
    return <CustomLoginDialogComponent setTokenAndHostname={setTokenAndHostname} authError={authError} />;
  } else {
    return <></>;
  }
}
