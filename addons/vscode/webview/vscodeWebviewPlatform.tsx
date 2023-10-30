/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ClientToServerAPI} from 'isl/src/ClientToServerAPI';
import type {Platform} from 'isl/src/platform';
import type {ThemeColor} from 'isl/src/theme';
import type {RepoRelativePath} from 'isl/src/types';
import type {Comparison} from 'shared/Comparison';

import {Internal} from './Internal';
import {globalRecoil} from 'isl/src/AccessGlobalRecoil';
import {getAllRecoilStateJson} from 'isl/src/debug/getAllRecoilStateJson';

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
  openDiff: (path: RepoRelativePath, comparison: Comparison) =>
    window.clientToServerAPI?.postMessage({type: 'platform/openDiff', path, comparison}),
  openExternalLink: url => {
    window.clientToServerAPI?.postMessage({type: 'platform/openExternal', url});
  },
  clipboardCopy: data => navigator.clipboard.writeText(data),

  getTemporaryState<T>(key: string): T | null {
    try {
      const found = localStorage.getItem(key) as string | null;
      if (found == null) {
        return null;
      }
      return JSON.parse(found) as T;
    } catch {
      return null;
    }
  },
  setTemporaryState<T>(key: string, value: T): void {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {}
  },

  theme: {
    getTheme,
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

  registerServerListeners: (api: ClientToServerAPI) => {
    return api.onMessageOfType('platform/getUiState', () => {
      const state = getAllRecoilStateJson(globalRecoil().getSnapshot());
      window.clientToServerAPI?.postMessage({
        type: 'platform/gotUiState',
        state: JSON.stringify(state, undefined, 2),
      });
    });
  },
  onCommitFormSubmit: Internal.onCommitFormSubmit,
};

function getTheme(): ThemeColor {
  return document.body.className.includes('vscode-light') ? 'light' : 'dark';
}
