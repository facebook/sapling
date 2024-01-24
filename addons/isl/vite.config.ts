/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import react from '@vitejs/plugin-react';
import {defineConfig} from 'vite';
import viteTsconfigPaths from 'vite-tsconfig-paths';

// TODO: this could be a glob on src/platform/*.html
const platforms = {
  main: 'index.html',
  androidStudio: 'androidStudio.html',
  androidStudioRemote: 'androidStudioRemote.html',
  standalone: 'standalone.html',
  webview: 'webview.html',
  chromelikeApp: 'chromelikeApp.html',
};

export default defineConfig({
  base: '',
  plugins: [
    react({
      include: '**/*.tsx',
    }),
    viteTsconfigPaths(),
  ],
  build: {
    outDir: 'build',
    manifest: true,
    rollupOptions: {
      input: platforms,
    },
  },
  server: {
    // No need to open the browser, it's opened by `yarn serve` in `isl-server`.
    open: false,
    port: 3000,
  },
});
