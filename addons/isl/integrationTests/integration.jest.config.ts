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

  // Even though the repos are separate, somehow running multiple integration tests in parallel
  // causes failures. For now, just limit to one test at a time.
  maxWorkers: 1,

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
  setupFilesAfterEnv: ['<rootDir>/integrationTests/setupTests.ts'],

  moduleNameMapper: {
    '\\.css$': '<rootDir>/src/__mocks__/styleMock.ts',
  },
};
export default config;
