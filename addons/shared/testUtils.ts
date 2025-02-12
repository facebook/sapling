/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MeasureMemoryOptions} from 'node:vm';
import type {Json} from './typeUtils';

import {measureMemory} from 'node:vm';
import {Logger} from '../isl-server/src/logger';

export class MockLogger extends Logger {
  write() {
    // noop
  }
}
export const mockLogger = new MockLogger();

export function clone<T extends Json>(o: T): T {
  return JSON.parse(JSON.stringify(o));
}

/**
 * Returns a Promise which resolves after the current async tick is finished.
 * Useful for testing code which `await`s.
 */
export function nextTick(): Promise<void> {
  return new Promise(res => setTimeout(res, 0));
}

export async function gc() {
  // 'node --expose-gc' defines 'global.gc'.
  // To run with yarn: yarn node --expose-gc ./node_modules/.bin/jest ...
  const globalGc = global.gc;
  if (globalGc != null) {
    await new Promise<void>(r =>
      setTimeout(() => {
        globalGc();
        r();
      }, 0),
    );
  } else {
    // measureMemory with 'eager' has a side effect of running the GC.
    // This exists since node 14.
    // 'as' used since `MeasureMemoryOptions` is outdated (node 13?).
    await measureMemory({execution: 'eager'} as MeasureMemoryOptions);
  }
}
