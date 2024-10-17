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

// Use __mocks__/logger so calls to logger don't output to console, but
// console.log still works for debugging tests.
jest.mock('./logger');

// jest doesn't have the stylex compilation step, let's just mock it
jest.mock('@stylexjs/stylex');

// Mock MessageBus via LocalWebSocketEventBus before other logic which might have effects on it.
jest.mock('./LocalWebSocketEventBus', () => {
  // eslint-disable-next-line @typescript-eslint/no-var-requires, @typescript-eslint/consistent-type-imports
  const TestMessageBus = (require('./TestingMessageBus') as typeof import('./TestingMessageBus'))
    .TestingEventBus;
  return {LocalWebSocketEventBus: TestMessageBus};
});

import {configure} from '@testing-library/react';

const IS_CI = !!process.env.SANDCASTLE || !!process.env.GITHUB_ACTIONS;
configure({
  // bump waitFor timeouts in CI where jobs may run slower
  ...(IS_CI ? {asyncUtilTimeout: 5_000} : undefined),
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

global.fetch = jest.fn().mockImplementation(() => Promise.resolve());
