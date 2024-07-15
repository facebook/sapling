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

// jest doesn't have the stylex compilation step, let's just mock it
jest.mock('@stylexjs/stylex', () => {
  return {
    defineVars(_: unknown) {
      return {};
    },
    createTheme(_: unknown, __: unknown) {
      return {};
    },
    props(..._: Array<unknown>) {
      return {};
    },
    create(o: unknown) {
      return o;
    },
  };
});

import resizeObserver from 'resize-observer-polyfill';
window.ResizeObserver = resizeObserver;
