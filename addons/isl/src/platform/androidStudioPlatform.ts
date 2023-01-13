/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from '../platform';
import type {RepoRelativePath} from '../types';

declare global {
  interface Window {
    __IdeBridge: {
      openFileInAndroidStudio: (path: string) => void;
      clipboardCopy?: (data: string) => void;
    };
  }
}

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated by webpack.

const androidStudioPlatform: Platform = {
  platformName: 'androidStudio',

  confirm: (message: string, details?: string) => {
    // TODO: Android Studio-style confirm modal
    const ok = window.confirm(message + '\n' + (details ?? ''));
    return Promise.resolve(ok);
  },

  openFile: (_path: RepoRelativePath) => {
    window.__IdeBridge.openFileInAndroidStudio(_path);
  },

  openExternalLink(_url: string): void {
    window.open(_url, '_blank');
  },

  clipboardCopy(data: string) {
    window.__IdeBridge.clipboardCopy?.(data);
  },
};

window.islPlatform = androidStudioPlatform;
