/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Context} from 'node:vm';
import type {OpenISLOptions, PageOptions} from './testBrowser';

import {join} from 'node:path';
import * as repl from 'node:repl';
import {TestBrowser} from './testBrowser';
import {TestRepo} from './testRepo';
import {getCacheDir} from './utils';

/** Reexport for convenience. */
export type {TestBrowser, TestRepo};

/** Defines an example - what does the repo looks like and what to do after opening ISL. */
export interface Example {
  // Actual examples might want to replace these fields:

  /** Prepare the test repo. */
  populateRepo(repo: TestRepo): Promise<void>;

  /** Logic to run after opening the ISL page. */
  postOpenISL(browser: TestBrowser, _repo: TestRepo): Promise<void>;

  /** Page options like what the initial viewport size is. */
  pageOptions(): PageOptions;

  /** Initial ISL options. */
  openISLOptions: OpenISLOptions;

  /** Repo configs. */
  repoConfigs: Record<string, string>;

  // Utilitity methods. Usually inherited as-is by actual examples:

  /** Create and populate a test repo. */
  createRepo(): Promise<TestRepo>;

  /** Create a test browser. */
  createBrowser(): Promise<TestBrowser>;

  /** Run this example. */
  run(): Promise<void>;

  /** Start REPL for debugging. */
  repl(): Promise<void>;
  browser?: TestBrowser;
  repo?: TestRepo;
}

const logger = console;

export const BASE_EXAMPLE: Example = {
  async populateRepo(repo: TestRepo): Promise<void> {
    await repo.drawdag(
      `
        P9
         : C3
         | :
         | C1
         |/
        P7
         :
         | B3
         | :
         | B1
         |/
        P5
         : A2
         | |
         | A1
         |/
        P3
         :
        P1
      `,
      `
        commit('A1', '[sl] windows: update Python', date='300h ago')
        commit('A2', 'debug', date='300h ago')
        commit('B1', '[eden] Thread EdenConfig down to Windows fsck', date='3d ago')
        commit('B2', '[eden] Remove n^2 path comparisons from Windows fsck', date='3d ago')
        commit('B3', '[edenfs] Recover Overlay from disk/scm for Windows fsck', date='3d ago')
        commit('C1', '[eden] Use PathMap for WindowsFsck', date='2d ago')
        commit('C2', '[eden] Close Windows file handle during Windows Fsck', date='2d ago')
        commit('C3', 'temp', date='2d ago')
        commit('C4', '[eden] Support long paths in Windows FSCK', date='12m ago')
        # Use different dates for public commits so ISL forceConnectPublic() can sort them.
        opts = {
            'P9': {'remotename': 'remote/main'},
            'P7': {'remotename': 'remote/stable', 'date': '48h ago'},
            'P6': {'pred': 'A1', 'op': 'land'},
            'P5': {'date': '73h ago'},
            'P3': {'date': '301h ago'},
        }
        date = '0h ago'
        for i in range(9, 0, -1):
            name = f'P{i}'
            kwargs = opts.get(name) or {}
            date = kwargs.pop('date', None) or date
            commit(name, date=date, **kwargs)
            date = str(int(date.split('h')[0]) + 1) + 'h ago'
      `,
    );
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
    now: 964785600, // 2000-7-28
  },
  repoConfigs: {
    'ui.username': 'Mary <mary@example.com>',
    'remotenames.selectivepulldefault': 'main',
    'smartlog.names': 'main,stable',
  },
  async createRepo(): Promise<TestRepo> {
    const repo = await TestRepo.new();
    const cacheKey = JSON.stringify([
      this.populateRepo.toString(),
      this.repoConfigs,
      this.openISLOptions.now,
      process.env.SL,
    ]);
    await repo.cached(async repo => {
      await repo.init();
      const configs = Object.entries(this.repoConfigs).map(([k, v]) => `${k}=${v}`);
      const now = this.openISLOptions.now;
      if (now != null) {
        configs.push(`devel.default-date=${now} 0`);
      }
      await repo.setConfig(configs);
      await this.populateRepo(repo);
    }, cacheKey);
    this.repo = repo;
    return repo;
  },
  async createBrowser(): Promise<TestBrowser> {
    const browser = await TestBrowser.new(this.pageOptions());
    this.browser = browser;
    return browser;
  },
  async run(): Promise<void> {
    // Both operations are slow. Run them in parallel.
    const [repo, browser] = await Promise.all([this.createRepo(), this.createBrowser()]);

    // Open ISL after the repo is populated.
    await browser.openISL(repo, this.openISLOptions);

    // Run example-defined extra logic.
    await this.postOpenISL(browser, repo);

    // Close the browser.
    logger.info('Closing browser');
    browser.browser.close();
  },
  async repl(): Promise<void> {
    // Start node REPL to play with Puppeteer internals.
    logger.info('REPL context:');
    const context: Context = {};
    const {browser, repo} = this;
    if (browser != null) {
      logger.info('- page: Puppeteer page object');
      logger.info('- browser: Puppeteer browser object');
      logger.info('- testBrowser: TestBrowser object');
      context.page = browser.page;
      context.browser = browser.browser;
      context.testBrowser = browser;
    }
    if (repo != null) {
      logger.info('- repo: TestRepo object');
      context.repo = repo;
    }

    const replServer = repl.start('> ');
    Object.assign(replServer.context, context);

    replServer.setupHistory(join(await getCacheDir('repl'), 'history'), () => {});

    // Wait for REPL exit.
    return new Promise<void>(resolve => {
      replServer.on('exit', () => {
        resolve();
      });
    });
  },
};
