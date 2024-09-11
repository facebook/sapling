/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {OpenISLOptions, PageOptions, TestBrowser} from './testBrowser';
import type {TestRepo} from './testRepo';

/** Reexport for convenience. */
export type {TestBrowser, TestRepo};

/** Defines an example - what does the repo looks like and what to do after opening ISL. */
export interface Example {
  /** Prepare the test repo. */
  populateRepo(repo: TestRepo): Promise<void>;

  /** Logic to run after opening the ISL page. */
  postOpenISL(browser: TestBrowser, _repo: TestRepo): Promise<void>;

  /** Page options like what the initial viewport size is. */
  pageOptions(): PageOptions;

  /** Initial ISL options. */
  openISLOptions: OpenISLOptions;
}

export const BASE_EXAMPLE: Example = {
  async populateRepo(repo: TestRepo): Promise<void> {
    await repo.cached(repo => repo.drawdag('A..D', `goto('D')`));
  },
  async postOpenISL(browser: TestBrowser, _repo: TestRepo): Promise<void> {
    await browser.page.screenshot({path: 'example.png'});
  },
  pageOptions(): PageOptions {
    return {
      width: this.openISLOptions.sidebarOpen ? 860 : 600,
      height: 500,
    };
  },
  openISLOptions: {
    lightTheme: true,
    sidebarOpen: false,
  },
};
