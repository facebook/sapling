/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import '@testing-library/jest-dom';

// Use __mocks__/logger so calls to logger don't output to console, but
// console.log still works for debugging tests.
jest.mock('./logger');
