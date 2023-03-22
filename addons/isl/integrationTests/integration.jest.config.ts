/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Config} from '@jest/types';

const config: Config.InitialOptions = {
  verbose: true,
  testRegex: '/integrationTests/.*\\.test\\.tsx?',

  // shutting down watchman is async, so our `dispose` does not synchronously tear down everything,
  // thus jest thinks we have leaked handles. Force exits quiets this warning.
  forceExit: true,

  // use typescript in tests
  preset: 'ts-jest',
  testEnvironment: 'jsdom',
  rootDir: '..',
  globals: {
    'ts-jest': {
      // don't do type checking in tests
      diagnostics: false,
    },
  },

  moduleNameMapper: {
    '\\.css$': '<rootDir>/integrationTests/__mocks__/styleMock.ts',
  },
};
export default config;
