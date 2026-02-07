/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {gitHubHostnameAtom, gitHubTokenPersistenceAtom} from './jotai';
import {useSetAtom} from 'jotai';

export type CustomLoginDialogProps = {
  setTokenAndHostname(token: string, hostname: string): void;
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
  function setTokenAndHostname(token: string, hostname: string): void {
    setHostname(hostname);
    setToken(token);
  }
  if (CustomLoginDialogComponent != null) {
    return <CustomLoginDialogComponent setTokenAndHostname={setTokenAndHostname} />;
  } else {
    return <></>;
  }
}
