/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Example, TestBrowser, TestRepo} from '../example';

import {BASE_EXAMPLE} from '../example';
import * as repl from 'node:repl';

const logger = console;

export const EXAMPLE: Example = {
  ...BASE_EXAMPLE,
  postOpenISL(browser: TestBrowser, repo: TestRepo): Promise<void> {
    // Start node REPL to play with Puppeteer internals.
    logger.info('REPL context:');
    logger.info('- page: Puppeteer page object');
    logger.info('- browser: Puppeteer browser object');
    logger.info('- testBrowser: TestBrowser object');
    logger.info('- repo: TestRepo object');

    const replServer = repl.start('> ');
    const context = replServer.context;
    context.page = browser.page;
    context.browser = browser.browser;
    context.testBrowser = browser;
    context.repo = repo;

    // Wait for REPL exit.
    return new Promise<void>(resolve => {
      replServer.on('exit', () => {
        resolve();
      });
    });
  },
};
