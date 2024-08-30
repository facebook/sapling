/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from 'isl/src/platform';
import type {ThemeColor} from 'isl/src/theme';
import type {RepoRelativePath} from 'isl/src/types';
import type {Comparison} from 'shared/Comparison';
import type {Json} from 'shared/typeUtils';

import {Internal} from './Internal';
import {browserPlatformImpl} from 'isl/src/platform/browerPlatformImpl';
import {registerCleanup} from 'isl/src/utils';
import {lazy} from 'react';

const VSCodeSettings = lazy(() => import('./VSCodeSettings'));
const AddMoreCwdsHint = lazy(() => import('./AddMoreCwdsHint'));

declare global {
  interface Window {
    islInitialPersistedState: Record<string, Json>;
  }
}

const persistedState: Record<string, Json> = window.islInitialPersistedState ?? {};

export const vscodeWebviewPlatform: Platform = {
  platformName: 'vscode',
  confirm: (message: string, details?: string | undefined) => {
    window.clientToServerAPI?.postMessage({type: 'platform/confirm', message, details});

    // wait for confirmation result
    return new Promise<boolean>(res => {
      const disposable = window.clientToServerAPI?.onMessageOfType(
        'platform/confirmResult',
        event => {
          res(event.result);
          disposable?.dispose();
        },
      );
    });
  },
  openFile: (path, options) =>
    window.clientToServerAPI?.postMessage({type: 'platform/openFile', path, options}),
  openFiles: (paths, options) =>
    window.clientToServerAPI?.postMessage({type: 'platform/openFiles', paths, options}),
  canCustomizeFileOpener: false,
  openDiff: (path: RepoRelativePath, comparison: Comparison) =>
    window.clientToServerAPI?.postMessage({type: 'platform/openDiff', path, comparison}),
  openExternalLink: url => {
    window.clientToServerAPI?.postMessage({type: 'platform/openExternal', url});
  },
  upsellExternalMergeTool: false,

  openDedicatedComparison: async (comparison: Comparison): Promise<boolean> => {
    const {getComparisonPanelMode} = await import('./state');
    const mode = getComparisonPanelMode();
    if (mode === 'Auto') {
      return false;
    }
    window.clientToServerAPI?.postMessage({
      type: 'platform/executeVSCodeCommand',
      command: 'sapling.open-comparison-view',
      args: [comparison],
    });
    return true;
  },

  clipboardCopy: browserPlatformImpl.clipboardCopy,

  getPersistedState<T extends Json>(key: string): T | null {
    return persistedState[key] as T;
  },
  setPersistedState<T extends Json>(key: string, value: T): void {
    persistedState[key] = value;

    window.clientToServerAPI?.postMessage({
      type: 'platform/setPersistedState',
      key,
      data: JSON.stringify(value),
    });
  },
  clearPersistedState(): void {
    for (const key in persistedState) {
      delete persistedState[key];
      window.clientToServerAPI?.postMessage({
        type: 'platform/setPersistedState',
        key,
        data: undefined,
      });
    }
  },
  getAllPersistedState(): Json | undefined {
    return persistedState;
  },

  theme: {
    getTheme,
    getThemeName: () => document.body.dataset.vscodeThemeId,
    resetCSS: '',
    onDidChangeTheme(callback: (theme: ThemeColor) => unknown) {
      // VS Code sets the theme inside the webview by adding a class to `document.body`.
      // Listen for changes to body to possibly update the theme value.
      // This also covers theme name changes, which might keep light / dark the same.
      const observer = new MutationObserver((_mutationList: Array<MutationRecord>) => {
        callback(getTheme());
      });
      observer.observe(document.body, {attributes: true, childList: false, subtree: false});
      return {dispose: () => observer.disconnect()};
    },
  },

  AdditionalDebugContent: Internal.AdditionalDebugContent,
  GettingStartedContent: Internal.GettingStartedContent,
  AddMoreCwdsHint,
  Settings: VSCodeSettings,
};

function getTheme(): ThemeColor {
  return document.body.className.includes('vscode-light') ? 'light' : 'dark';
}

/**
 * VS Code has a bug where it will lose focus on webview elements (notably text areas) when tabbing out and back in.
 * To mitigate, we save the currently focused element on window blur, and refocus it on window focus.
 */
let lastTextAreaBeforeBlur: HTMLElement | null = null;

const handleWindowFocus = () => {
  const lastTextArea = lastTextAreaBeforeBlur;
  lastTextArea?.focus?.();
};
const handleWindowBlur = () => {
  if (document.activeElement == document.body) {
    // Blur can get called with document.body as document.activeElement after focusing an inner element.
    // Ignore these, as refocusing document.body is not useful.
    return;
  }
  // Save the last thing that had focus, which is focusable
  if (
    document.activeElement == null ||
    (document.activeElement as HTMLElement | null)?.focus != null
  ) {
    lastTextAreaBeforeBlur = document.activeElement as HTMLElement | null;
  }
};
window.addEventListener('focus', handleWindowFocus);
window.addEventListener('blur', handleWindowBlur);
registerCleanup(
  vscodeWebviewPlatform,
  () => {
    window.removeEventListener('focus', handleWindowFocus);
    window.removeEventListener('blur', handleWindowBlur);
  },
  import.meta.hot,
);
