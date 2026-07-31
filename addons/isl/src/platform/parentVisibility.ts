/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PlatformVisibility} from '../platform';
import {isPageVisibility} from '../platformVisibility';
import type {PageVisibility} from '../types';

export const PARENT_VISIBILITY_VERSION = 1;

export function makeParentVisibilitySource(targetWindow: Window): PlatformVisibility {
  let visibility: PageVisibility = 'focused';
  const listeners = new Set<(next: PageVisibility) => unknown>();

  const handleMessage = (event: MessageEvent) => {
    if (event.source !== targetWindow.parent) {
      return;
    }
    const message = event.data as
      {type?: unknown; version?: unknown; visibility?: unknown} | null | undefined;
    if (
      message?.type !== 'isl/platform/visibility/set' ||
      message.version !== PARENT_VISIBILITY_VERSION ||
      !isPageVisibility(message.visibility)
    ) {
      return;
    }
    const next = message.visibility;
    if (next !== visibility) {
      visibility = next;
      for (const listener of listeners) {
        listener(next);
      }
    }
    targetWindow.parent.postMessage(
      {
        type: 'isl/platform/visibility/ack',
        version: PARENT_VISIBILITY_VERSION,
        visibility: next,
      },
      '*',
    );
  };

  targetWindow.addEventListener('message', handleMessage);
  targetWindow.parent.postMessage(
    {
      type: 'isl/platform/visibility/ready',
      version: PARENT_VISIBILITY_VERSION,
      graphicsContextCount: null,
    },
    '*',
  );

  return {
    getVisibility: () => visibility,
    onDidChangeVisibility: callback => {
      listeners.add(callback);
      return {dispose: () => listeners.delete(callback)};
    },
  };
}
