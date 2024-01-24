/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ApplicationInfo} from './types';

import os from 'os';
import {randomId, unwrap} from 'shared/utils';

export function getUsername(): string {
  try {
    return os.userInfo().username;
  } catch (osInfoError) {
    try {
      const {env} = process;
      return unwrap(env.LOGNAME || env.USER || env.LNAME || env.USERNAME);
    } catch (processEnvError) {
      throw new Error(String(processEnvError) + String(osInfoError));
    }
  }
}

export function generateAnalyticsInfo(
  platformName: string,
  version: string,
  sessionId?: string,
): ApplicationInfo {
  return {
    platform: platformName,
    version,
    repo: undefined,
    /**
     * Random id for this ISL session, created at startup.
     * Note: this is only generated on the server, so client-logged events share the ID with the server.
     * May be manually specified instead of randomly created, e.g. if your platform has a well-defined session ID already.
     */
    sessionId: sessionId ?? randomId(),
    unixname: getUsername(),
    osArch: os.arch(),
    osType: os.platform(),
    osRelease: os.release(),
    hostname: os.hostname(),
  };
}
