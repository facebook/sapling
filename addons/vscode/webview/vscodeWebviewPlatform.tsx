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
import {tryJsonParse} from 'shared/utils';

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
  canCustomizeFileOpener: false,
  openDiff: (path: RepoRelativePath, comparison: Comparison) =>
    window.clientToServerAPI?.postMessage({type: 'platform/openDiff', path, comparison}),
  openExternalLink: url => {
    window.clientToServerAPI?.postMessage({type: 'platform/openExternal', url});
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
    resetCSS: '',
    onDidChangeTheme(callback: (theme: ThemeColor) => unknown) {
      let lastValue = getTheme();
      // VS Code sets the theme inside the webview by adding a class to `document.body`.
      // Listen for changes to body to possibly update the theme value.
      const observer = new MutationObserver((_mutationList: Array<MutationRecord>) => {
        const newValue = getTheme();
        if (lastValue !== newValue) {
          callback(newValue);
          lastValue = newValue;
        }
      });
      observer.observe(document.body, {attributes: true, childList: false, subtree: false});
      return {dispose: () => observer.disconnect()};
    },
  },

  AdditionalDebugContent: Internal.AdditionalDebugContent,
  GettingStartedContent: Internal.GettingStartedContent,
  GettingStartedBugNuxContent: Internal.GettingStartedBugNuxContent,
};

function getTheme(): ThemeColor {
  return document.body.className.includes('vscode-light') ? 'light' : 'dark';
}
