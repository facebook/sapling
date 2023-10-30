/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ClientToServerAPI} from './ClientToServerAPI';
import type {ThemeColor} from './theme';
import type {
  Disposable,
  OneIndexedLineNumber,
  PlatformName,
  RepoRelativePath,
  ServerToClientMessage,
} from './types';
import type {LazyExoticComponent} from 'react';
import type {Comparison} from 'shared/Comparison';
import type {Json} from 'shared/typeUtils';

import {browserPlatform} from './BrowserPlatform';

export type InitialParamKeys = 'token' | string;

/**
 * Platform-specific API for each target: vscode extension, electron standalone, browser, ...
 */
export interface Platform {
  platformName: PlatformName;
  confirm(message: string, details?: string): Promise<boolean>;
  openFile(path: RepoRelativePath, options?: {line?: OneIndexedLineNumber}): void;
  openContainingFolder?(path: RepoRelativePath): void;
  openDiff?(path: RepoRelativePath, comparison: Comparison): void;
  openExternalLink(url: string): void;
  clipboardCopy(value: string): void;
  chooseFile?(title: string, multi: boolean): Promise<Array<File>>;
  onCommitFormSubmit?: () => void;
  /**
   * Get stored data from local persistant cache (usually browser local storage).
   * Note: Some platforms may not support this (e.g. browser with localStorage disabled),
   * or it may not be persisted indefinitely---usual localStorage caveats apply.
   */
  getTemporaryState<T extends Json>(key: string): T | null;
  /** see getTemporaryState  */
  setTemporaryState<T extends Json>(key: string, value: T): void;

  handleServerMessage?: (message: ServerToClientMessage) => void;

  /** Called once (if provided) when the ClientToServerAPI is initialized,
   * so platform-specific server events can be listened to.
   */
  registerServerListeners?: (api: ClientToServerAPI) => Disposable;

  /**
   * Component representing additional buttons/info in the help menu.
   * Note: This should be lazy-loaded via `React.lazy()` so that implementations
   * may import any files without worrying about the platform being set up yet or not.
   */
  AdditionalDebugContent?: LazyExoticComponent<() => JSX.Element>;
  /**
   * Content to show in splash screen when starting ISL for the first time.
   * Note: This should be lazy-loaded via `React.lazy()` so that implementations
   * may import any files without worrying about the platform being set up yet or not.
   */
  GettingStartedContent?: LazyExoticComponent<({dismiss}: {dismiss: () => void}) => JSX.Element>;
  /** Content to show as a tooltip on the bug button after going through the getting started experience */
  GettingStartedBugNuxContent?: LazyExoticComponent<
    ({dismiss}: {dismiss: () => void}) => JSX.Element
  >;

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
