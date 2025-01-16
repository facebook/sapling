/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// jest-dom adds custom jest matchers for asserting on DOM nodes.
// allows you to do things like:
// expect(element).toHaveTextContent(/react/i)
// learn more: https://github.com/testing-library/jest-dom
import '@testing-library/jest-dom';

/* eslint-disable no-console */
global.console = require('console');

jest.mock('@stylexjs/stylex');

// reduce flakiness by retrying
jest.retryTimes(1);

import {configure} from '@testing-library/react';

const IS_CI = !!process.env.SANDCASTLE || !!process.env.GITHUB_ACTIONS;
configure({
  // bump waitFor timeouts in CI where jobs may run slower
  asyncUtilTimeout: IS_CI ? 30_000 : 20_000,
  ...(process.env.HIDE_RTL_DOM_ERRORS
    ? {
        getElementError: (message: string | null) => {
          const error = new Error(message ?? '');
          error.name = 'TestingLibraryElementError';
          error.stack = undefined;
          return error;
        },
      }
    : {}),
});

global.ResizeObserver = require('resize-observer-polyfill');
