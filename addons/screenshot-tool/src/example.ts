/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TestBrowser} from './testBrowser';
import type {TestRepo} from './testRepo';

/** Reexport for convenience. */
export type {TestBrowser, TestRepo};

/** Defines an example - what does the repo looks like and what to do after opening ISL. */
export interface Example {
  /** Prepare the test repo. */
  populateRepo(repo: TestRepo): Promise<void>;

  /** Logic to run after opening the ISL page. */
  postOpenISL(browser: TestBrowser, _repo: TestRepo): Promise<void>;
}

export const BASE_EXAMPLE: Example = {
  async populateRepo(repo: TestRepo): Promise<void> {
    await repo.drawdag('A..D', `goto('D')`);
  },
  async postOpenISL(browser: TestBrowser, _repo: TestRepo): Promise<void> {
    await browser.page.screenshot({path: 'example.png'});
  },
};
