/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from '../platform';
import type {ThemeColor} from '../theme';
import type {OneIndexedLineNumber, RepoRelativePath} from '../types';

import {browserPlatformImpl} from './browerPlatformImpl';

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
// since it will end up getting duplicated when bundling.

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
  openFiles: (paths: Array<RepoRelativePath>, _options: {line?: OneIndexedLineNumber}) => {
    for (const path of paths) {
      // TODO: support line numbers
      window.__IdeBridge.openFileInAndroidStudio(path);
    }
  },
  canCustomizeFileOpener: false,
  upsellExternalMergeTool: false,

  openExternalLink(_url: string): void {
    window.open(_url, '_blank');
  },

  clipboardCopy(text: string, _html?: string) {
    window.__IdeBridge.clipboardCopy?.(text);
  },

  getPersistedState: browserPlatformImpl.getPersistedState,
  setPersistedState: browserPlatformImpl.setPersistedState,
  clearPersistedState: browserPlatformImpl.clearPersistedState,
  getAllPersistedState: browserPlatformImpl.getAllPersistedState,

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

// Load the actual app entry, which must be done after the platform has been set up.
import('../index');
