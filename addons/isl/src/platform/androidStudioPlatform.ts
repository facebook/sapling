/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from '../platform';
import type {ThemeColor} from '../theme';
import type {OneIndexedLineNumber, RepoRelativePath} from '../types';

import {makeBrowserLikePlatformImpl} from './browserPlatformImpl';

declare global {
  interface Window {
    __IdeBridge: {
      openFileInAndroidStudio: (path: string, line?: number, col?: number) => void;
      clipboardCopy?: (data: string) => void;
      getIDETheme(): ThemeColor;
    };
  }
}

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated when bundling.

const androidStudioPlatform: Platform = {
  ...makeBrowserLikePlatformImpl('androidStudio'),

  confirm: (message: string, details?: string) => {
    // TODO: Android Studio-style confirm modal
    const ok = window.confirm(message + '\n' + (details ?? ''));
    return Promise.resolve(ok);
  },

  openFile: (_path: RepoRelativePath, _options?: {line?: OneIndexedLineNumber}) => {
    window.__IdeBridge.openFileInAndroidStudio(_path, _options?.line);
  },
  openFiles: (paths: Array<RepoRelativePath>, _options?: {line?: OneIndexedLineNumber}) => {
    for (const path of paths) {
      window.__IdeBridge.openFileInAndroidStudio(path, _options?.line);
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
