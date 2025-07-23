/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/* eslint-disable no-await-in-loop */

import type {Browser, Page} from 'puppeteer-core';
import type * as Rrweb from 'rrweb';
import type * as testRepo from './testRepo';

import fs from 'node:fs';
import {dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import puppeteer from 'puppeteer-core';
import {getCacheDir, sha1} from './utils';

export type PageOptions = {
  width?: number;
  height?: number;
};

export type OpenISLOptions = {
  lightTheme: boolean;
  sidebarOpen: boolean;
  /** UTC unixtime for relative dates, in seconds */
  now?: number;
};

const logger = console;

const __dirname = dirname(fileURLToPath(import.meta.url));
const nodeModulesDir = join(__dirname, '..', 'node_modules');

/**
 * Controls a test browser via Puppeteer.
 * Add convenient methods to interact with the browser here.
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
    let url = await repo.serveUrl();
    if (options.now != null) {
      // The query param is in milliseconds.
      url += `&now=${options.now * 1000}`;
    }
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

  async prepareRecording(): Promise<void> {
    await this.page.addScriptTag({
      path: join(nodeModulesDir, 'rrweb', 'dist', 'rrweb.umd.cjs'),
    });
    // No need to prepare again.
    this.prepareRecording = () => Promise.resolve();
  }

  /**
   * Start recording using rrweb.
   * Use `stopRecording` to stop and obtain recorded events.
   */
  async startRecording(): Promise<void> {
    await this.prepareRecording();
    await this.page.evaluate(() => {
      const browserWindow = window as unknown as BrowserWindow;
      const rrweb = browserWindow.rrweb;
      const events: Array<Rrweb.eventWithTime> = [];
      browserWindow.recordedEvents = events;
      browserWindow.stopRecording = rrweb.record({
        emit(event) {
          events.push(event);
        },
        inlineImages: true,
        collectFonts: true,
        slimDOMOptions: {
          script: true,
          comment: true,
          headFavicon: true,
        },
      });
    });
  }

  /** Stop recording. Return the recorded objects for reply. */
  async stopRecording(): Promise<Array<Rrweb.eventWithTime>> {
    const events = await this.page.evaluate(() => {
      const browserWindow = window as unknown as BrowserWindow;
      browserWindow.stopRecording?.();
      return browserWindow.recordedEvents ?? [];
    });
    // https://cdn.jsdelivr.net/npm/@vscode/codicons@0.0.36/dist/codicon.ttf
    // Also backup the events to a local file.
    const eventsDir = await getCacheDir('events');
    const data = JSON.stringify(events);
    const eventsPath = `${eventsDir}/${Date.now()}-${sha1(data).substring(0, 6)}.json`;
    logger.info(`Backing up events to ${eventsPath}`);
    await fs.promises.writeFile(eventsPath, data);
    return events;
  }

  /** Open a new tab to replay recorded events. */
  async replayInNewTab(
    events: Array<Rrweb.eventWithTime>,
    options?: Partial<Rrweb.playerConfig>,
  ): Promise<void> {
    const page = await this.browser.newPage();
    let eventsStr = JSON.stringify(events);
    // Fix up URL to codicon.ttf.
    eventsStr = eventsStr.replace(
      /http:\/\/localhost:[0-9]*\/assets\/codicon-[a-zA-Z0-9]*\.ttf/,
      'https://cdn.jsdelivr.net/npm/@vscode/codicons@0.0.36/dist/codicon.ttf',
    );
    const html = `
<!DOCTYPE html>
<html>
  <head><title>rrweb replay</title></head>
  <body></body>
  <script>
    function startReplay() {
      const events = ${eventsStr};
      const player = new rrwebPlayer({
        target: document.body,
        props: {
          events,
          mouseTail: false,
          skipInactive: true,
          inactivePeriodThreshold: 2000,
          ...${JSON.stringify(options ?? {})}
        }
      });
    }
  </script>
</html>
    `;
    const width = 1600;
    const height = 1200;
    await page.setViewport({width, height});
    await page.setContent(html);
    await page.addStyleTag({
      path: join(nodeModulesDir, 'rrweb-player', 'dist', 'style.css'),
    });
    await page.addScriptTag({
      path: join(nodeModulesDir, 'rrweb-player', 'dist', 'index.js'),
    });
    await page.evaluate('startReplay()');
  }

  constructor(
    public browser: Browser,
    public page: Page,
  ) {}
}

function sleep(ms: number): Promise<void> {
  return new Promise(r => setTimeout(r, ms));
}

type BrowserWindow = {
  rrweb: typeof Rrweb;
  stopRecording?: () => void;
  recordedEvents?: Array<Rrweb.eventWithTime>;
};
