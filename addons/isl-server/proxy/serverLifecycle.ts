/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ServerChallengeResponse} from './server';

import {type ExistingServerInfo, readExistingServerFile} from './existingServerStateFiles';
import {areTokensEqual} from './proxyUtils';
import * as http from 'http';

/**
 * If it looks like something is serving on `localhost` on the same port,
 * send it a request to verify it's actually ISL.
 * Send it the token we recovered and then validate it responds with
 * the same challenge token we recovered.
 *
 * If the challenge is successful, returns the PID of the server; otherwise,
 * returns null.
 */
export async function checkIfServerIsAliveAndIsISL(
  info: typeof console.info,
  port: number,
  existingServerInfo: ExistingServerInfo,
  silent = false,
): Promise<number | null> {
  let response;
  try {
    const result = await Promise.race<string>([
      new Promise<string>((res, rej) => {
        const req = http.request(
          {
            hostname: 'localhost',
            port,
            path: `/challenge_authenticity?token=${existingServerInfo.sensitiveToken}`,
            method: 'GET',
          },
          response => {
            response.on('data', d => {
              res(d);
            });
            response.on('error', e => {
              rej(e);
            });
          },
        );
        req.on('error', rej);
        req.end();
      }),
      // Timeout so we don't wait around forever for it.
      // This should always be on localhost and therefore quite fast.
      new Promise<never>((_, rej) => setTimeout(() => rej('timeout'), 500)),
    ]);

    response = JSON.parse(result) as ServerChallengeResponse;
  } catch (error) {
    if (!silent) {
      info(`error checking if existing Sapling Web server on port ${port} is authentic: `, error);
    }
    // if the request fails for any reason, we don't think it's an ISL server.
    return null;
  }

  const {challengeToken, pid} = response;
  return areTokensEqual(challengeToken, existingServerInfo.challengeToken) ? pid : null;
}

/**
 * Try multiple times to read the server data, in case we try to read during the time between the server
 * starting and it writing to  the token file.
 */
export async function readExistingServerFileWithRetries(
  port: number,
): Promise<ExistingServerInfo | undefined> {
  let tries = 3;
  while (tries > 0) {
    try {
      // eslint-disable-next-line no-await-in-loop
      return await readExistingServerFile(port);
    } catch (error) {
      sleepMs(500);
    }
    tries--;
  }
  return undefined;
}

function sleepMs(timeMs: number): Promise<void> {
  return new Promise(res => setTimeout(res, timeMs));
}
