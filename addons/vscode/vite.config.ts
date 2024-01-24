/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import react from '@vitejs/plugin-react';
import {defineConfig} from 'vite';
import viteTsconfigPaths from 'vite-tsconfig-paths';

export default defineConfig({
  base: '',
  plugins: [
    react({
      include: '**/*.tsx',
    }),
    viteTsconfigPaths(),
  ],
  build: {
    outDir: 'dist/webview',
    manifest: true,
    rollupOptions: {
      input: 'webview.html',
      output: {
        // Don't use hashed names, so ISL webview panel can pre-define what filename to load
        entryFileNames: '[name].js',
        chunkFileNames: '[name].js',
        assetFileNames: 'res/[name].[ext]',
      },
    },
    copyPublicDir: true,
    sourcemap: true,
  },
  publicDir: '../isl/public',
  server: {
    // No need to open the browser, we run inside vscode and don't really connect to the server.
    open: false,
  },
});
