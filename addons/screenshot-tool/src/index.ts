/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Example} from './example';

import {TestBrowser} from './testBrowser';
import {TestRepo} from './testRepo';

const logger = console;

async function loadExample(name: string): Promise<Example> {
  logger.info('Loaded example:', name);
  const example = await import(`./examples/${name}.ts`);
  return example.EXAMPLE as Example;
}

export async function main() {
  const name = process.argv.at(2) ?? 'repl';
  const example = await loadExample(name);

  const createTestRepo = async () => {
    const repo = await TestRepo.new();
    await example.populateRepo(repo);
    return repo;
  };

  // Both operations are slow. Run them in parallel.
  const [repo, browser] = await Promise.all([
    createTestRepo(),
    TestBrowser.new(example.pageOptions()),
  ]);

  // Open ISL after the repo is populated.
  await browser.openISL(repo, example.openISLOptions);

  // Run example-defined extra logic.
  await example.postOpenISL(browser, repo);

  // Close the browser.
  logger.info('Closing browser');
  browser.browser.close();
}

main();
