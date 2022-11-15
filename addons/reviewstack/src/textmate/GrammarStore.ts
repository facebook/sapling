/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {IGrammar, IRawTheme, Registry} from 'vscode-textmate';

import createTextMateRegistry from './createTextMateRegistry';
import {loadWASM} from 'vscode-oniguruma';

/**
 * The site that hosts the ReviewStack UI must make onig.wasm available on
 * the host at this path.
 */
const URL_TO_ONIG_WASM = '/generated/textmate/onig.wasm';

export default class GrammarStore {
  private constructor(private registry: Registry) {}

  static async createGrammarStore(theme: IRawTheme): Promise<GrammarStore> {
    await ensureOnigurumaIsLoaded();
    const registry = createTextMateRegistry(theme);
    return new GrammarStore(registry);
  }

  loadGrammar(initialScopeName: string): Promise<IGrammar | null> {
    return this.registry.loadGrammar(initialScopeName);
  }

  getColorMap(): string[] {
    return this.registry.getColorMap();
  }
}

let onigurumaLoadingJob: Promise<void> | null = null;

function ensureOnigurumaIsLoaded(): Promise<void> {
  if (onigurumaLoadingJob === null) {
    onigurumaLoadingJob = loadOniguruma();
  }

  return onigurumaLoadingJob;
}

async function loadOniguruma(): Promise<void> {
  const onigurumaWASMRequest = fetch(URL_TO_ONIG_WASM);
  const response = await onigurumaWASMRequest;

  const contentType = response.headers.get('content-type');
  const useStreamingParser = contentType === 'application/wasm';

  if (useStreamingParser) {
    await loadWASM(response);
  } else {
    const dataOrOptions = {
      data: await response.arrayBuffer(),
      print(str: string): void {
        // eslint-disable-next-line no-console
        console.info(str);
      },
    };
    await loadWASM(dataOrOptions);
  }
}
