/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/* eslint-disable no-await-in-loop */

import type * as testRepo from './testRepo';
import type {Browser, Page} from 'puppeteer-core';

import fs from 'node:fs';
import puppeteer from 'puppeteer-core';

type PageOptions = {
  width?: number;
  height?: number;
};

export type OpenISLOptions = {
  lightTheme: boolean;
  sidebarOpen: boolean;
};

const logger = console;

/**
 * Controls a test browser via Puppeteer.
 * Add convinent methods to interact with the browser here.
 */
export class TestBrowser {
  /**
   * Spawns a new test browser and opens a tab for testing.
   * The browser and page can be accessed via the `browser` and `page` properties.
   */
  static async new(opts?: PageOptions): Promise<TestBrowser> {
    const browserPath =
      process.env.BROWSER ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
    if (fs.existsSync(browserPath) === false) {
      throw new Error(`Browser path ${browserPath} does not exist`);
    }
    logger.info('Launching browser');
    const browser = await puppeteer.launch({
      headless: false,
      executablePath: browserPath,
    });
    const pages = await browser.pages();
    const page = pages.at(0) ?? (await browser.newPage());
    const {width = 1600, height = 900} = opts ?? {};
    await page.setViewport({width, height});
    return new TestBrowser(browser, page);
  }

  /** Wait until all spinners (.codicon-loading) are gone. */
  async waitForSpinners(): Promise<void> {
    const page = this.page;
    logger.debug('Waiting for spinners');
    let noSpinnerCount = 0;
    while (true) {
      // eslint-disable-next-line no-await-in-loop
      const spinner = await page.$('.codicon-loading');
      if (spinner == null) {
        noSpinnerCount += 1;
        if (noSpinnerCount > 3) {
          logger.debug('Waited for spinners');
          return;
        }
      } else {
        noSpinnerCount = 0;
      }
      // eslint-disable-next-line no-await-in-loop
      await sleep(100);
    }
  }

  /** Open the ISL page for a repo. */
  async openISL(repo: testRepo.TestRepo, options: OpenISLOptions): Promise<void> {
    const url = await repo.serveUrl();
    logger.info(`Opening ${url}`);
    await this.page.goto(url, {waitUntil: 'networkidle2'});
    await this.waitForSpinners();
    await this.setSidebarOpen(options.sidebarOpen);
    await this.setLightTheme(options.lightTheme);
  }

  /** Toggle light/dark theme by pressing 'T' */
  async setLightTheme(lightTheme: boolean): Promise<void> {
    // Sometimes the first key press doesn't work, so try again once.
    for (let i = 0; i < 2; i++) {
      const current = await this.hasElement('.light-theme');
      if (current === lightTheme) {
        return;
      }
      logger.debug('Setting lightTheme', lightTheme);
      const keyboard = this.page.keyboard;
      await keyboard.down('AltLeft');
      await keyboard.press('T');
      await keyboard.up('AltLeft');
    }
  }

  /** Toggle sidebar. */
  async setSidebarOpen(sidebarOpen: boolean): Promise<void> {
    const current = await this.hasElement('.drawer-right.drawer-expanded');
    if (current === sidebarOpen) {
      return;
    }
    logger.debug('Setting sidebarOpen', sidebarOpen);
    await this.page.click('.drawer-label');
  }

  async hasElement(selector: string): Promise<boolean> {
    return (await this.page.$(selector)) != null;
  }

  constructor(public browser: Browser, public page: Page) {}
}

function sleep(ms: number): Promise<void> {
  return new Promise(r => setTimeout(r, ms));
}
