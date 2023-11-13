/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from '../platform';
import type {ThemeColor} from '../theme';
import type {OneIndexedLineNumber, RepoRelativePath} from '../types';

declare global {
  interface Window {
    __IdeBridge: {
      openFileInAndroidStudio: (path: string) => void;
      clipboardCopy?: (data: string) => void;
      getIDETheme(): ThemeColor;
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

  openFile: (_path: RepoRelativePath, _options: {line?: OneIndexedLineNumber}) => {
    // TODO: support line numbers
    window.__IdeBridge.openFileInAndroidStudio(_path);
  },

  openExternalLink(_url: string): void {
    window.open(_url, '_blank');
  },

  clipboardCopy(data: string) {
    window.__IdeBridge.clipboardCopy?.(data);
  },

  getTemporaryState<T>(_key: string): T | null {
    // TODO: support local storage, which may require enabling some webview permissions.
    return null;
  },
  setTemporaryState<T>(_key: string, _value: T): void {
    // TODO: support local storage, which may require enabling some webview permissions.
  },

  theme: {
    getTheme(): ThemeColor {
      return 'dark'; // default to dark, IDE will adjust the theme if necessary
    },
    onDidChangeTheme(callback: (theme: ThemeColor) => unknown) {
      const updateTheme = (data: CustomEvent<ThemeColor>) => {
        callback(data.detail);
      };

      window.addEventListener('onIDEThemeChange', updateTheme as EventListener, false);

      return {
        dispose: () => {
          window.removeEventListener('onIDEThemeChange', updateTheme as EventListener, false);
        },
      };
    },
  },
};

window.islPlatform = androidStudioPlatform;
