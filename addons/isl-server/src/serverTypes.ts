/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ServerSideTracker} from './analytics/serverSideTracker';
import type {Logger} from './logger';

/** Context in which to execute sl commands */
export type ExecutionContext = {
  cmd: string;
  cwd: string;
  logger: Logger;
  tracker: ServerSideTracker;
};
