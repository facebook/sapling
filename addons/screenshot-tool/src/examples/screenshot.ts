/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Example, TestBrowser, TestRepo} from '../example';

import {BASE_EXAMPLE} from '../example';

export const EXAMPLE: Example = {
  ...BASE_EXAMPLE,
  async postOpenISL(browser: TestBrowser, _repo: TestRepo): Promise<void> {
    await browser.page.screenshot({path: 'example.png'});
  },
};
