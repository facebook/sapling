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

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated when bundling.

const obsidianPlatform: Platform = {
  ...makeBrowserLikePlatformImpl('obsidian'),

  // Override file opening to send messages to Obsidian via postMessage
  openFile: (path: RepoRelativePath, options?: {line?: OneIndexedLineNumber}) => {
    window.parent.postMessage(
      {
        type: 'isl/platform/openFile',
        path,
        line: options?.line,
      },
      '*',
    );
  },

  openFiles: (paths: ReadonlyArray<RepoRelativePath>, options?: {line?: OneIndexedLineNumber}) => {
    window.parent.postMessage(
      {
        type: 'isl/platform/openFiles',
        paths,
        line: options?.line,
      },
      '*',
    );
  },

  canCustomizeFileOpener: false, // Obsidian controls file opening
  upsellExternalMergeTool: false, // Obsidian is the editor

  openExternalLink(url: string): void {
    window.parent.postMessage(
      {
        type: 'isl/platform/openExternal',
        url,
      },
      '*',
    );
  },

  // Theme integration
  theme: {
    getTheme(): ThemeColor {
      // Default to dark, will be updated by Obsidian
      return 'dark';
    },

    onDidChangeTheme(callback: (theme: ThemeColor) => unknown) {
      const handleMessage = (event: MessageEvent) => {
        if (event.data?.type === 'obsidian/themeChanged') {
          const theme: ThemeColor = event.data.theme === 'dark' ? 'dark' : 'light';
          callback(theme);
        }
      };

      window.addEventListener('message', handleMessage);

      return {
        dispose: () => {
          window.removeEventListener('message', handleMessage);
        },
      };
    },
  },
};

window.islPlatform = obsidianPlatform;

// Debug: Log when platform is initialized
console.log('[ISL Obsidian] Platform initialized');

// Forward all server messages to Obsidian parent window for event logging
// This allows the Obsidian plugin to monitor all ISL server events
obsidianPlatform.messageBus.onMessage(event => {
  console.log('[ISL Obsidian] Received server message');
  try {
    const data = JSON.parse(event.data as string);
    window.parent.postMessage(
      {
        type: 'isl/serverMessage',
        data,
      },
      '*',
    );
  } catch (e) {
    console.log('[ISL Obsidian] Failed to parse message:', e);
  }
});

// Debug: Log before importing index
console.log('[ISL Obsidian] About to import index');

// Load the actual app entry, which must be done after the platform has been set up.
import('../index')
  .then(() => {
    console.log('[ISL Obsidian] Index imported successfully');
  })
  .catch(e => {
    console.error('[ISL Obsidian] Failed to import index:', e);
  });
