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

// jsdom does not implement scrollIntoView; stub it so components that call it
// (e.g. auto-scroll-to-"You are here") don't throw during tests.
window.HTMLElement.prototype.scrollIntoView = jest.fn();

// jsdom does not implement IntersectionObserver; stub it so components that observe
// (e.g. the "You are here" viewport tracking) don't throw during tests.
global.IntersectionObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
  takeRecords() {
    return [];
  }
} as unknown as typeof IntersectionObserver;

global.fetch = jest.fn().mockImplementation(() => Promise.resolve());

// Default all QE flags to false in tests so they don't hang waiting for server responses
beforeEach(() => {
  // Use lazy require() to avoid loading featureFlags (and its transitive deps like i18n)
  // at module level, which would prevent test-specific jest.mock() calls from taking effect.
  // eslint-disable-next-line @typescript-eslint/no-var-requires
  const {__TEST__: featureFlagTestUtils} = require('./featureFlags');
  featureFlagTestUtils.enableQeFlagOverrides();
});
