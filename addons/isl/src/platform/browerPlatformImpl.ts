/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Json} from 'shared/typeUtils';
import type {Platform} from '../platform';
import type {OneIndexedLineNumber, PlatformName, RepoRelativePath} from '../types';

import {LocalWebSocketEventBus} from '../LocalWebSocketEventBus';
import {computeInitialParams} from '../urlParams';

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated when bundling.

export function browserClipboardCopy(text: string, html?: string) {
  if (html) {
    const htmlBlob = new Blob([html], {type: 'text/html'});
    const textBlob = new Blob([text], {type: 'text/plain'});
    const clipboardItem = new window.ClipboardItem({
      'text/html': htmlBlob,
      'text/plain': textBlob,
    });
    navigator.clipboard.write([clipboardItem]);
  } else {
    navigator.clipboard.writeText(text);
  }
}

export const makeBrowserLikePlatformImpl = (platformName: PlatformName): Platform => {
  const initialUrlParams = computeInitialParams(platformName === 'browser');
  return {
    platformName,
    confirm: (message: string, details?: string) => {
      const ok = window.confirm(message + '\n' + (details ?? ''));
      return Promise.resolve(ok);
    },

    openFile: (path: RepoRelativePath, options?: {line?: OneIndexedLineNumber}) => {
      window.clientToServerAPI?.postMessage({type: 'platform/openFile', path, options});
    },
    openFiles: (paths: Array<RepoRelativePath>, options?: {line?: OneIndexedLineNumber}) => {
      window.clientToServerAPI?.postMessage({type: 'platform/openFiles', paths, options});
    },
    canCustomizeFileOpener: true,
    upsellExternalMergeTool: true,

    openContainingFolder: (path: RepoRelativePath) => {
      window.clientToServerAPI?.postMessage({type: 'platform/openContainingFolder', path});
    },

    openExternalLink(url: string): void {
      window.open(url, '_blank');
    },

    getPersistedState<T>(key: string): T | null {
      try {
        const found = localStorage.getItem(key) as string | null;
        if (found == null) {
          return null;
        }
        return JSON.parse(found) as T;
      } catch {
        return null;
      }
    },
    setPersistedState<T>(key: string, value: T | undefined): void {
      try {
        if (value === undefined) {
          localStorage.removeItem(key);
        } else {
          localStorage.setItem(key, JSON.stringify(value));
        }
      } catch {}
    },
    clearPersistedState(): void {
      try {
        localStorage.clear();
      } catch {}
    },
    getAllPersistedState(): Json | undefined {
      try {
        return Object.fromEntries(
          Object.entries({...localStorage})
            .map(([key, value]: [string, unknown]) => {
              try {
                return [key, JSON.parse(value as string)];
              } catch {
                return null;
              }
            })
            .filter((e): e is [string, Json] => e != null),
        );
      } catch {
        return undefined;
      }
    },

    clipboardCopy: browserClipboardCopy,

    messageBus: new LocalWebSocketEventBus(
      process.env.NODE_ENV === 'development'
        ? // in dev mode, Vite hosts our files for hot-reloading.
          // This means we can't host the ws server on the same port as the page.
          'localhost:3001'
        : // in production, we serve both the static files and ws from the same port
          location.host,
      WebSocket,
      {
        cwd: initialUrlParams.get('cwd'),
        sessionId: initialUrlParams.get('sessionId'),
        token: initialUrlParams.get('token'),
        platformName,
      },
    ),

    initialUrlParams,
  };
};
