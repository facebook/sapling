/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {ejeca} from 'shared/ejeca';
import {Internal} from '../Internal';
import {isEjecaError, isEjecaSpawnError} from '../utils';

/** Max retries for transient failures (rate limits, server errors). */
const MAX_RETRIES = 2;
/** Base delay between retries in ms. Doubled on each attempt. */
const RETRY_BASE_DELAY_MS = 1_000;

/**
 * Check if a gh CLI failure is transient and worth retrying.
 * Looks at stderr/stdout for rate limit, server error, or network patterns.
 */
function isTransientFailure(error: unknown): boolean {
  if (!isEjecaError(error)) {
    return false;
  }
  const text = `${error.stderr ?? ''} ${error.stdout ?? ''}`.toLowerCase();
  return (
    text.includes('rate limit') ||
    text.includes('abuse detection') ||
    text.includes('secondary rate') ||
    text.includes('502') ||
    text.includes('503') ||
    text.includes('server error') ||
    text.includes('timedout') ||
    text.includes('econnreset') ||
    text.includes('socket hang up') ||
    text.includes('try again later')
  );
}

/**
 * Extract a human-readable error message from a gh CLI failure.
 * Parses stderr/stdout for GraphQL errors or HTTP error messages.
 */
function extractGhErrorDetail(error: unknown): string | undefined {
  if (!isEjecaError(error)) {
    return undefined;
  }
  // gh often puts the actual error on stderr
  const stderr = error.stderr?.trim();
  const stdout = error.stdout?.trim();

  // Try to parse GraphQL errors from stdout (gh sometimes exits 1 but still outputs JSON)
  if (stdout) {
    try {
      const json = JSON.parse(stdout);
      if (Array.isArray(json.errors) && json.errors.length > 0) {
        return json.errors.map((e: {message?: string}) => e.message).join('; ');
      }
    } catch {
      // not JSON, ignore
    }
  }

  // stderr often has the most useful message
  if (stderr) {
    return stderr;
  }

  return undefined;
}

export type QueryGraphQLOptions = {
  timeoutMs?: number;
  /** Signal to abort the request early (e.g. when navigating away from a PR). */
  signal?: AbortSignal;
};

export default async function queryGraphQL<TData, TVariables>(
  query: string,
  variables: TVariables,
  hostname: string,
  timeoutOrOptions?: number | QueryGraphQLOptions,
): Promise<TData> {
  // Support both old (number) and new (options object) signatures
  const options: QueryGraphQLOptions =
    typeof timeoutOrOptions === 'number'
      ? {timeoutMs: timeoutOrOptions}
      : timeoutOrOptions ?? {};

  if (Object.prototype.hasOwnProperty.call(variables, 'query')) {
    throw Error('cannot have a variable named query');
  }

  const args = ['api', 'graphql'];
  for (const [key, value] of Object.entries(variables as unknown as {[key: string]: unknown})) {
    if (value === undefined) {
      continue;
    }
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
      case 'object':
        // Arrays and objects (e.g. DraftPullRequestReviewThread[]) are
        // serialized as JSON.  The gh CLI's -F flag parses the value,
        // so JSON arrays/objects are passed through to GraphQL correctly.
        args.push('-F', `${key}=${JSON.stringify(value)}`);
        break;
      default:
        throw Error(`unexpected type: ${type} for ${key}: ${value}`);
    }
  }
  args.push('--hostname', hostname);
  args.push('-f', `query=${query}`);

  let lastError: unknown;

  for (let attempt = 0; attempt <= MAX_RETRIES; attempt++) {
    // Check abort before each attempt
    if (options.signal?.aborted) {
      throw new Error('GraphQL request aborted');
    }

    if (attempt > 0) {
      const delay = RETRY_BASE_DELAY_MS * Math.pow(2, attempt - 1);
      // eslint-disable-next-line no-console
      console.log(`[queryGraphQL] Retry attempt ${attempt}/${MAX_RETRIES} after ${delay}ms`);
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(resolve, delay);
        // If aborted during wait, cancel and reject
        options.signal?.addEventListener(
          'abort',
          () => {
            clearTimeout(timer);
            reject(new Error('GraphQL request aborted'));
          },
          {once: true},
        );
      });
    }

    let timedOut = false;
    let onAbort: (() => void) | undefined;

    try {
      const proc = ejeca('gh', args, {
        env: {
          ...((await Internal.additionalGhEnvVars?.()) ?? {}),
        },
      });

      // Kill the process if our signal is aborted
      onAbort = () => {
        proc.kill('SIGTERM', {forceKillAfterTimeout: 5_000});
      };
      options.signal?.addEventListener('abort', onAbort, {once: true});

      // TODO: move this into ejeca itself
      let timeoutId: NodeJS.Timeout | undefined;
      if (options.timeoutMs != null && options.timeoutMs > 0) {
        timeoutId = setTimeout(() => {
          proc.kill('SIGTERM', {forceKillAfterTimeout: 5_000});
          timedOut = true;
        }, options.timeoutMs);
        proc.on('exit', () => {
          clearTimeout(timeoutId);
        });
      }

      const {stdout} = await proc;
      options.signal?.removeEventListener('abort', onAbort);

      const json = JSON.parse(stdout);

      if (Array.isArray(json.errors)) {
        const msg = json.errors.map((e: {message?: string}) => e.message).join('; ');
        throw new Error(`GraphQL error: ${msg}`);
      }

      return json.data;
    } catch (error: unknown) {
      lastError = error;
      // Always clean up abort listener
      if (onAbort) {
        options.signal?.removeEventListener('abort', onAbort);
      }

      if (options.signal?.aborted) {
        throw new Error('GraphQL request aborted');
      }

      if (isEjecaSpawnError(error)) {
        if (error.code === 'ENOENT' || error.code === 'EACCES') {
          // `gh` not installed on path — not transient, don't retry
          throw new Error(`GhNotInstalledError: ${(error as Error).stack}`);
        }
      } else if (isEjecaError(error)) {
        // Log stderr/stdout from gh for debugging
        if (error.stderr) {
          // eslint-disable-next-line no-console
          console.error(`[queryGraphQL] gh stderr: ${error.stderr}`);
        }
        if (error.stdout) {
          // eslint-disable-next-line no-console
          console.error(`[queryGraphQL] gh stdout: ${error.stdout}`);
        }

        if (error.exitCode === 4) {
          // `gh` CLI exit code 4 => authentication issue — not transient
          throw new Error(`NotAuthenticatedError: ${(error as Error).stack}`);
        }

        // Retry transient failures
        if (attempt < MAX_RETRIES && isTransientFailure(error)) {
          continue;
        }

        // Not retryable or out of retries — throw with the actual error detail
        const detail = extractGhErrorDetail(error);
        if (detail) {
          throw new Error(`GitHub API error: ${detail}`);
        }
      }

      if (timedOut) {
        throw new Error(`TimedOutError: ${(error as Error).stack}`);
      }
      throw error;
    }
  }

  // Should not reach here, but just in case
  throw lastError;
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
    await ejeca('gh', args, {
      env: {
        ...((await Internal.additionalGhEnvVars?.()) ?? {}),
      },
    });
    return true;
  } catch {
    return false;
  }
}
