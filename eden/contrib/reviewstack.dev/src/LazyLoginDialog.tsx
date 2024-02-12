/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import React, {Suspense} from 'react';

const DefaultLoginDialog = React.lazy(() => import('./DefaultLoginDialog'));
const NetlifyLoginDialog = React.lazy(() => import('./NetlifyLoginDialog'));

export default function LazyLoginDialog({
  setTokenAndHostname,
}: {
  setTokenAndHostname: (token: string, hostname: string) => void;
}) {
  const {hostname} = window.location;
  const LoginComponent =
    hostname === 'reviewstack.netlify.app' || hostname === 'reviewstack.dev'
      ? NetlifyLoginDialog
      : DefaultLoginDialog;

  return (
    <div>
      <Suspense fallback={<div>Loading...</div>}>
        <LoginComponent setTokenAndHostname={setTokenAndHostname} />
      </Suspense>
    </div>
  );
}
