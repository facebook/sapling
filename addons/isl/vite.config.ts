/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// import babel from '@rolldown/plugin-babel';
import react from '@vitejs/plugin-react';
import fs from 'node:fs';
import path, {resolve} from 'node:path';
import {defineConfig} from 'vite';

// Normalize `c:\foo\index.html` to `c:/foo/index.html`.
// This affects Rolldown's `facadeModuleId` (which expects the `c:/foo/bar` format),
// and is important for Vite to replace the script tags in HTML files.
// See https://github.com/vitejs/vite/blob/7440191715b07a50992fcf8c90d07600dffc375e/packages/vite/src/node/plugins/html.ts#L804
// Without this, building on Windows might produce HTML entry points with
// missing `<script>` tags, resulting in a blank page.
function normalizeInputPath(inputPath: string) {
  return process.platform === 'win32' ? resolve(inputPath).replace(/\\/g, '/') : inputPath;
}

// TODO: this could be a glob on src/platform/*.html
const platforms = {
  main: normalizeInputPath('index.html'),
  androidStudio: normalizeInputPath('androidStudio.html'),
  androidStudioRemote: normalizeInputPath('androidStudioRemote.html'),
  webview: normalizeInputPath('webview.html'),
  chromelikeApp: normalizeInputPath('chromelikeApp.html'),
  visualStudio: normalizeInputPath('visualStudio.html'),
  obsidian: normalizeInputPath('obsidian.html'),
  agentHome: normalizeInputPath('agentHome.html'),
};

export default defineConfig({
  base: '',
  resolve: {
    tsconfigPaths: true,
  },
  plugins: [
    react(),
    // babel({
    //   plugins: [
    //     [
    //       'jotai/babel/plugin-debug-label',
    //       {
    //         customAtomNames: [
    //           'atomFamilyWeak',
    //           'atomLoadableWithRefresh',
    //           'atomWithOnChange',
    //           'atomWithRefresh',
    //           'atomLoadableWithRefresh',
    //           'atomResetOnCwdChange',
    //           'atomResetOnDepChange',
    //           'configBackedAtom',
    //           'jotaiAtom',
    //           'lazyAtom',
    //           'localStorageBackedAtom',
    //         ],
    //       },
    //     ],
    //     'jotai/babel/plugin-react-refresh',
    //   ],
    // }),
    // The manifest vite generates doesn't include web worker js files.
    // Just output a simple list of all files that are produced,
    // and the server can serve those known files.
    {
      name: 'emit-file-list',
      apply: 'build',
      writeBundle(options: {dir: string | undefined}, bundle: Record<string, unknown>) {
        const outputDir = options.dir || 'dist';
        const fileList = Object.keys(bundle)
          .filter(file => file !== '.vite/manifest.json')
          .map(p => p.replace(/\\/g, '/'));
        fs.writeFileSync(
          path.join(outputDir, 'assetList.json'),
          JSON.stringify(fileList, undefined, 2),
        );
      },
    },
  ],
  build: {
    outDir: 'build',
    rolldownOptions: {
      input: platforms,
    },
  },
  server: {
    // No need to open the browser, it's opened by `yarn serve` in `isl-server`.
    open: false,
    port: 3000,
  },
});
