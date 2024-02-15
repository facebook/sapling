/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoRelativePath, OneIndexedLineNumber} from '../types';
import type {Json} from 'shared/typeUtils';

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated when bundling.

export const browserPlatformImpl = {
  confirm: (message: string, details?: string) => {
    const ok = window.confirm(message + '\n' + (details ?? ''));
    return Promise.resolve(ok);
  },

  openFile: (path: RepoRelativePath, options?: {line?: OneIndexedLineNumber}) => {
    window.clientToServerAPI?.postMessage({type: 'platform/openFile', path, options});
  },

  openContainingFolder: (path: RepoRelativePath) => {
    window.clientToServerAPI?.postMessage({type: 'platform/openContainingFolder', path});
  },

  openExternalLink(url: string): void {
    window.open(url, '_blank');
  },

  clipboardCopy(data: string): void {
    navigator.clipboard.writeText(data);
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
  setPersistedState<T>(key: string, value: T): void {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {}
  },
  clearPersistedState(): void {
    try {
      localStorage.clear();
    } catch {}
  },
  getAllPersistedState(): Json | undefined {
    try {
      return {...localStorage};
    } catch {
      return undefined;
    }
  },
};
