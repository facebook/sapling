/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Comparison} from '../../../shared/Comparison';
import type {Platform} from '../platform';
import type {OneIndexedLineNumber, RepoRelativePath} from '../types';

import {makeBrowserLikePlatformImpl} from './browserPlatformImpl';

declare global {
  interface Window {
    __vsIdeBridge: {
      openFileInVisualStudio: (path: string, line?: number, col?: number) => void;
      openDiffInVisualStudio: (path: string, comparison: Comparison) => void;
    };
  }
}

const visualStudioPlatform: Platform = {
  ...makeBrowserLikePlatformImpl('visualStudio'),

  confirm: (message: string, details?: string) => {
    const ok = window.confirm(message + '\n' + (details ?? ''));
    return Promise.resolve(ok);
  },

  openFile: async (path: RepoRelativePath, options?: {line?: OneIndexedLineNumber}) => {
    if (window.__vsIdeBridge && window.__vsIdeBridge.openFileInVisualStudio) {
      const helpers = await import('./platformHelpers');
      const repoRoot = helpers.getRepoRoot();
      if (repoRoot) {
        const fullPath = `${repoRoot}/${path}`;
        window.__vsIdeBridge.openFileInVisualStudio(fullPath, options?.line);
      }
    }
  },
  openFiles: async (paths: Array<RepoRelativePath>, _options?: {line?: OneIndexedLineNumber}) => {
    if (window.__vsIdeBridge && window.__vsIdeBridge.openFileInVisualStudio) {
      const helpers = await import('./platformHelpers');
      const repoRoot = helpers.getRepoRoot();
      if (repoRoot) {
        for (const path of paths) {
          const fullPath = `${repoRoot}/${path}`;
          window.__vsIdeBridge.openFileInVisualStudio(fullPath, _options?.line);
        }
      }
    }
  },
  openDiff: async (path: RepoRelativePath, comparison: Comparison) => {
    if (window.__vsIdeBridge && window.__vsIdeBridge.openDiffInVisualStudio) {
      const helpers = await import('./platformHelpers');
      const repoRoot = helpers.getRepoRoot();
      if (repoRoot) {
        const fullPath = `${repoRoot}/${path}`;
        window.__vsIdeBridge.openDiffInVisualStudio(fullPath, comparison);
      }
    }
  },
  canCustomizeFileOpener: false,
  upsellExternalMergeTool: false,

  openExternalLink(_url: string): void {
    window.open(_url, '_blank');
  },
};

window.islPlatform = visualStudioPlatform;

// Load the actual app entry, which must be done after the platform has been set up.
import('../index');
