/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TextMateGrammar} from 'shared/textmate-lib/types';

export default async function fetchGrammar(
  moduleName: string,
  type: 'json' | 'plist',
): Promise<TextMateGrammar> {
  const uri = `/generated/textmate/${moduleName}.${type}`;
  const response = await fetch(uri);
  const grammar = await response.text();
  return {type, grammar};
}
