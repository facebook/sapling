/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {gitHubTokenPersistence} from './github/gitHubCredentials';
import {useSetRecoilState} from 'recoil';

export type CustomLoginDialogProps = {
  setToken(token: string): void;
};

let CustomLoginDialogComponent: React.FunctionComponent<CustomLoginDialogProps> | null = null;

export function setCustomLoginDialogComponent(
  component: React.FunctionComponent<CustomLoginDialogProps>,
) {
  CustomLoginDialogComponent = component;
}

export default function LoginDialog(): React.ReactElement {
  const setToken = useSetRecoilState(gitHubTokenPersistence);
  if (CustomLoginDialogComponent != null) {
    return <CustomLoginDialogComponent setToken={setToken} />;
  } else {
    return <></>;
  }
}
