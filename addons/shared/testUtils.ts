/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from '../isl-server/src/logger';

export const mockLogger: Logger = {
  log: jest.fn(),
  info: jest.fn(),
  warn: jest.fn(),
  error: jest.fn(),
};

type json = string | number | boolean | null | json[] | {[key: string]: json};
export function clone<T extends json>(o: T): T {
  return JSON.parse(JSON.stringify(o));
}

/**
 * Returns a Promise which resolves after the current async tick is finished.
 * Useful for testing code which `await`s.
 */
export function nextTick(): Promise<void> {
  return new Promise(res => setTimeout(res, 0));
}
