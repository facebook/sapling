/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {IGrammar, Registry} from 'vscode-textmate';

export default class GrammarStore {
  /**
   * See `createTextMateRegistry()` in this directory to create a Registry.
   */
  constructor(private registry: Registry) {}

  /**
   * Load the grammar for `initialScopeName` and all referenced included
   * grammars asynchronously.
   */
  loadGrammar(initialScopeName: string): Promise<IGrammar | null> {
    return this.registry.loadGrammar(initialScopeName);
  }

  /**
   * Returns a lookup array for color ids.
   */
  getColorMap(): string[] {
    return this.registry.getColorMap();
  }
}
