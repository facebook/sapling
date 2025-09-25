/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {LazyExoticComponent} from 'react';
import type {Comparison} from 'shared/Comparison';
import type {Json} from 'shared/typeUtils';
import type {MessageBus} from './MessageBus';
import type {ThemeColor} from './theme';
import type {
  AbsolutePath,
  Disposable,
  OneIndexedLineNumber,
  PlatformName,
  RepoRelativePath,
  ServerToClientMessage,
} from './types';

import {browserPlatform} from './BrowserPlatform';
import type {CodeReviewIssue} from './firstPassCodeReview/types';

export type InitialParamKeys = 'token' | string;

/**
 * Platform-specific API for each target: vscode extension, electron standalone, browser, ...
 */
export interface Platform {
  platformName: PlatformName;
  confirm(message: string, details?: string): Promise<boolean>;
  openFile(path: RepoRelativePath, options?: {line?: OneIndexedLineNumber}): void;
  openFiles(paths: ReadonlyArray<RepoRelativePath>, options?: {line?: OneIndexedLineNumber}): void;
  canCustomizeFileOpener: boolean;
  openContainingFolder?(path: RepoRelativePath): void;
  openDiff?(path: RepoRelativePath, comparison: Comparison): void;
  openExternalLink(url: string): void;
  clipboardCopy(text: string, html?: string): void;
  chooseFile?(title: string, multi: boolean): Promise<Array<File>>;
  /** Whether to ask to configure an external merge tool. Useful for standalone platforms, but not embedded ones like vscode. */
  upsellExternalMergeTool: boolean;
  /**
   * Get stored data from local persistent cache (usually browser local storage).
   * Note: Some platforms may not support this (e.g. browser with localStorage disabled),
   * or it may not be persisted indefinitely---usual localStorage caveats apply.
   */
  getPersistedState<T extends Json>(key: string): T | null;
  /** see getPersistedState  */
  setPersistedState<T extends Json>(key: string, value: T | undefined): void;
  /** see getPersistedState  */
  clearPersistedState(): void;
  /** see getPersistedState  */
  getAllPersistedState(): Json | undefined;

  handleServerMessage?: (message: ServerToClientMessage) => void;

  openDedicatedComparison?: (comparison: Comparison) => Promise<boolean>;

  /**
   * Component representing additional buttons/info in the cwd menu,
   * used to show a button or hint about how to add more cwds.
   * Note: This should be lazy-loaded via `React.lazy()` so that implementations
   * may import any files without worrying about the platform being set up yet or not.
   */
  AddMoreCwdsHint?: LazyExoticComponent<() => JSX.Element>;

  /** Platform-specific settings, such as how ISL panels work */
  Settings?: LazyExoticComponent<() => JSX.Element>;

  theme?: {
    getTheme(): ThemeColor;
    getThemeName?(): string | undefined;
    onDidChangeTheme(callback: (theme: ThemeColor) => unknown): Disposable;
    resetCSS?: string;
  };

  /** If the platform has a notion of pending edits (typically from an AI), methods for listening and resolving them. */
  suggestedEdits?: {
    /** listen for changes to edits so ISL can confirm edits before taking actions. */
    onDidChangeSuggestedEdits(callback: (suggestedEdits: Array<AbsolutePath>) => void): Disposable;
    /** Accepts/Rejects edits */
    resolveSuggestedEdits(action: 'accept' | 'reject', files?: Array<AbsolutePath>): void;
  };

  messageBus: MessageBus;
  /** In browser-like platforms, some ISL parameters are passed via URL query params */
  initialUrlParams?: Map<InitialParamKeys, string>;

  /** If the platform has a notion of AI code review, methods for listening to them. */
  aiCodeReview?: {
    /** listen for new comments so ISL can render them */
    onDidChangeAIReviewComments(
      callback: (aiReviewComments: Array<CodeReviewIssue>) => void,
    ): Disposable;
  };
}

declare global {
  interface Window {
    islPlatform?: Platform;
  }
}

// [!] NOTE: On some platforms (vscode), this file is replaced at bundle time with a platform-specific implementation
// of the Platform interface.
// This file should have no other side effects than exporting the platform.

// However, non-vscode but non-browser platforms are defined by setting window.islPlatform
// before the main ISL script loads.

/** The ISL client Platform. This may be BrowserPlatform, VSCodeWebviewPlatform, or another platforms, determined at runtime.  */
const platform = window.islPlatform ?? browserPlatform;
window.islPlatform = platform;

export default platform;
