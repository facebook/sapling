/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import react from '@vitejs/plugin-react';
import {defineConfig} from 'vite';
import styleX from 'vite-plugin-stylex';
import viteTsconfigPaths from 'vite-tsconfig-paths';

export default defineConfig({
  base: '',
  plugins: [react(), styleX(), viteTsconfigPaths()],
  build: {
    outDir: 'build',
  },
});
