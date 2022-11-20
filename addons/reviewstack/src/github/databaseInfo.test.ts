/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DB_NAME, ALL_DB_NAMES_EVER} from './databaseInfo';

describe('databaseInfo', () => {
  // The goal of this test is to catch the case where some one updates
  // DB_VERSION or DB_NAME, but forgets to update ALL_DB_NAMES_EVER.
  test('ensure DB_NAME is in ALL_DB_NAMES_EVER', () => {
    expect(ALL_DB_NAMES_EVER.includes(DB_NAME)).toBe(true);
  });
});
