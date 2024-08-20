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
import {logger} from 'isl/src/logger';
import {browserPlatformImpl} from 'isl/src/platform/browerPlatformImpl';
import {registerCleanup} from 'isl/src/utils';
import {lazy} from 'react';
import {tryJsonParse} from 'shared/utils';

const VSCodeSettings = lazy(() => import('./VSCodeSettings'));

declare global {
  interface Window {
    islInitialPersistedState: Record<string, Json>;
  }
}

/**
 * Previously, persisted storage was backed by localStorage.
 * This is unreliable in vscode. Instead, we use extension storage.
 * If you previously used localStorage, let's load from there
 * initially, then clear localStorage to migrate.
 *
 * After this has rolled out to everyone, it's safe to delete this.
 */
function tryGetStateFromLocalStorage(): Record<string, Json> | undefined {
  const state: Record<string, Json> = {};
  try {
    const found = {...localStorage};
    for (const key in found) {
      state[key] = tryJsonParse(found[key] as string) ?? null;
    }
    if (localStorage.length > 0) {
      // If we found localStorage, save it as persisted storage instead, then clear localStorage.
      // We do this in a timeout because the clientToServerAPI is not initialized statically when this is run.
      persistStateAsSoonAsPossible();
      logger.info(
        'Found initial state in localStorage. Saving to extension storage instead; clearing localStorage.',
      );
      localStorage.clear();
    }
    return state;
  } catch (e) {
    return undefined;
  }
}

const persistedState: Record<string, Json> =
  window.islInitialPersistedState ?? tryGetStateFromLocalStorage() ?? {};

function persistStateAsSoonAsPossible() {
  let tries = 20;
  const persist = () => {
    if (window.clientToServerAPI == null) {
      if (tries-- > 0) {
        setTimeout(persist, 100);
      }
      return;
    }
    window.clientToServerAPI?.postMessage({
      type: 'platform/setPersistedState',
      data: JSON.stringify(persistedState),
    });
    logger.info('Saved persisted state to extension storage');
  };
  setTimeout(persist, 10);
}

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

    // send entire state every time
    window.clientToServerAPI?.postMessage({
      type: 'platform/setPersistedState',
      data: JSON.stringify(persistedState),
    });
  },
  clearPersistedState(): void {
    for (const key in persistedState) {
      delete persistedState[key];
    }
    window.clientToServerAPI?.postMessage({
      type: 'platform/setPersistedState',
      data: undefined,
    });
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
