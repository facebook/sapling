/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Example} from './example';

const logger = console;

async function loadExample(name: string): Promise<Example> {
  logger.info('Loaded example:', name);
  const example = await import(`./examples/${name}.ts`);
  return example.EXAMPLE as Example;
}

export async function main() {
  const name = process.argv.at(2) ?? 'repl';
  const example = await loadExample(name);
  await example.run();
}

main();
