/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from './platform';
import type {RepoRelativePath} from './types';

import serverAPI from './ClientToServerAPI';

declare global {
  interface Window {
    isSaplingVSCodeExtension?: boolean;
  }
}

export const browserPlatform: Platform = {
  platformName: 'browser',
  confirm: (message: string, details?: string) => {
    const ok = window.confirm(message + '\n' + (details ?? ''));
    return Promise.resolve(ok);
  },

  openFile: (path: RepoRelativePath) => {
    serverAPI.postMessage({type: 'platform/openFile', path});
  },

  openExternalLink(url: string): void {
    window.open(url, '_blank');
  },

  clipboardCopy(data: string): void {
    navigator.clipboard.writeText(data);
  },
};
