/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ThemeColor} from '../../theme';
import type {TextMateGrammar} from 'shared/textmate-lib/types';
import type {IGrammar, Registry} from 'vscode-textmate';

import {grammars} from '../../generated/textmate/TextMateGrammarManifest';
import VSCodeDarkPlusTheme from './VSCodeDarkPlusTheme';
import VSCodeLightPlusTheme from './VSCodeLightPlusTheme';
import createTextMateRegistry from 'shared/textmate-lib/createTextMateRegistry';
import {nullthrows} from 'shared/utils';

const grammarCache: Map<string, Promise<IGrammar | null>> = new Map();
export function getGrammar(store: Registry, scopeName: string): Promise<IGrammar | null> {
  if (grammarCache.has(scopeName)) {
    return nullthrows(grammarCache.get(scopeName));
  }
  const grammarPromise = store.loadGrammar(scopeName);
  grammarCache.set(scopeName, grammarPromise);
  return grammarPromise;
}

async function fetchGrammar(
  moduleName: string,
  type: 'json' | 'plist',
  base: string,
): Promise<TextMateGrammar> {
  const uri = new URL(`./generated/textmate/${moduleName}.${type}`, base);
  const response = await fetch(uri);
  const grammar = await response.text();
  return {type, grammar};
}

let cachedGrammarStore: {value: Registry; theme: ThemeColor} | null = null;
export function getGrammerStore(
  theme: ThemeColor,
  base: string,
  onNewColors?: (colorMap: string[]) => void,
) {
  const found = cachedGrammarStore;
  if (found != null && found.theme === theme) {
    return found.value;
  }

  // Grammars were cached according to the store, but the theme may have changed. Just bust the cache
  // to force grammars to reload.
  grammarCache.clear();

  const themeValues = theme === 'light' ? VSCodeLightPlusTheme : VSCodeDarkPlusTheme;

  const registry = createTextMateRegistry(themeValues, grammars, fetchGrammar, base);

  onNewColors?.(registry.getColorMap());

  cachedGrammarStore = {value: registry, theme};
  return registry;
}
