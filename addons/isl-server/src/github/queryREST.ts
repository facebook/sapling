/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {ejeca} from 'shared/ejeca';
import {Internal} from '../Internal';

export default async function queryREST<T>(
  endpoint: string,
  hostname: string,
  method: 'GET' | 'POST' = 'GET',
  payload?: Record<string, unknown>,
): Promise<T> {
  const args = ['api', endpoint, '--hostname', hostname, '--method', method];
  if (payload != null) {
    args.push('--input', '-');
  }

  const {stdout} = await ejeca('gh', args, {
    env: {
      ...((await Internal.additionalGhEnvVars?.()) ?? {}),
    },
    input: payload == null ? undefined : JSON.stringify(payload),
  });
  return JSON.parse(stdout) as T;
}
