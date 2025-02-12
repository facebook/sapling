/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import alias from '@rollup/plugin-alias';
import cjs from '@rollup/plugin-commonjs';
import importJson from '@rollup/plugin-json';
import nodeResolve from '@rollup/plugin-node-resolve';
import replace from '@rollup/plugin-replace';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import esbuild from 'rollup-plugin-esbuild';

// eslint-disable-next-line no-undef
const isProduction = process.env.NODE_ENV === 'production';

const filePath = fileURLToPath(import.meta.url);
const __dirname = path.dirname(filePath);
const projectRootDir = path.dirname(__dirname);

const customResolver = nodeResolve({
  extensions: ['.ts', '.tsx', '.mjs', '.js', '.jsx', '.json', '.sass', '.scss'],
});

export default (async () => {
  /** @type {import('rollup').RollupOptions} */
  return {
    input: './extension/extension.ts',
    output: {
      format: 'cjs',
      dir: 'dist',
      paths: id => id,
      sourcemap: true,
    },
    external: ['ws', 'vscode'],
    plugins: [
      replace({
        'process.env.NODE_ENV': isProduction ? '"production"' : '"development"',
        preventAssignment: true,
      }),
      // Support importing from `isl` and `shared` inside `vscode`
      alias({
        entries: [
          {
            find: /^isl/,
            replacement: path.resolve(projectRootDir, 'isl'),
          },
          {
            find: /^shared/,
            replacement: path.resolve(projectRootDir, 'shared'),
          },
        ],
        customResolver,
      }),
      esbuild(),
      nodeResolve({preferBuiltins: true, moduleDirectories: ['..', 'node_modules']}),
      cjs(),
      importJson(),
      isProduction && (await import('@rollup/plugin-terser')).default(),
    ],
  };
})();
