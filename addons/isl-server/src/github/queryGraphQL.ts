/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ExecaError} from 'execa';

import execa from 'execa';

export default async function queryGraphQL<TData, TVariables>(
  query: string,
  variables: TVariables,
  hostname: string,
): Promise<TData> {
  if (Object.prototype.hasOwnProperty.call(variables, 'query')) {
    throw Error('cannot have a variable named query');
  }

  const args = ['api', 'graphql'];
  for (const [key, value] of Object.entries(variables as unknown as {[key: string]: unknown})) {
    const type = typeof value;
    switch (type) {
      case 'boolean':
        args.push('-F', `${key}=${value}`);
        break;
      case 'number':
        args.push('-F', `${key}=${value}`);
        break;
      case 'string':
        args.push('-f', `${key}=${value}`);
        break;
      default:
        throw Error(`unexpected type: ${type} for ${key}: ${value}`);
    }
  }
  args.push('--hostname', hostname);
  args.push('-f', `query=${query}`);

  const {stdout} = await execa('gh', args, {stdout: 'pipe', stderr: 'pipe'}).catch(
    (error: ExecaError & {code?: string}) => {
      if (error.code === 'ENOENT' || error.code === 'EACCES') {
        // `gh` not installed on path
        throw new Error(`GhNotInstalledError: ${(error as Error).stack}`);
      } else if (error.exitCode === 4) {
        // `gh` CLI exit code 4 => authentication issue
        throw new Error(`NotAuthenticatedError: ${(error as Error).stack}`);
      }
      throw error;
    },
  );
  const json = JSON.parse(stdout);

  if (Array.isArray(json.errors)) {
    return Promise.reject(`Error: ${json.errors[0].message}`);
  }

  return json.data;
}

/**
 * Query `gh` CLI to test if a hostname is GitHub or GitHub Enterprise.
 * Returns true if this hostname is a valid, authenticated GitHub instance.
 * Returns false if the hostname is not github, or if you're not authenticated for that hostname,
 * or if the network is not working.
 */
export async function isGithubEnterprise(hostname: string): Promise<boolean> {
  const args = ['auth', 'status'];
  args.push('--hostname', hostname);

  try {
    await execa('gh', args, {stdout: 'pipe', stderr: 'pipe'});
    return true;
  } catch {
    return false;
  }
}
