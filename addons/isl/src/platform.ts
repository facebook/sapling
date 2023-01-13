/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ThemeColor} from './theme';
import type {Disposable, PlatformName, RepoRelativePath} from './types';

import {browserPlatform} from './BrowserPlatform';

export type InitialParamKeys = 'token' | string;

/**
 * Platform-specific API for each target: vscode extension, electron standalone, browser, ...
 */
export interface Platform {
  platformName: PlatformName;
  confirm(message: string, details?: string): Promise<boolean>;
  openFile(path: RepoRelativePath): void;
  openExternalLink(url: string): void;
  clipboardCopy(value: string): void;

  theme?: {
    getTheme(): ThemeColor;
    onDidChangeTheme(callback: (theme: ThemeColor) => unknown): Disposable;
  };
}

declare global {
  interface Window {
    islPlatform?: Platform;
  }
}

// Non-browser platforms are defined by setting window.islPlatform
// before the main ISL script loads.
const foundPlatform = window.islPlatform ?? browserPlatform;
window.islPlatform = foundPlatform;

export default foundPlatform;
