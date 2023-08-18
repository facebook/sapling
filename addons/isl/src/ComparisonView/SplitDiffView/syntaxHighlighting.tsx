/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ThemeColor} from '../../theme';
import type {ParsedDiff} from 'shared/patch/parse';
import type {HighlightedToken} from 'shared/textmate-lib/tokenize';
import type {TextMateGrammar} from 'shared/textmate-lib/types';
import type {Registry} from 'vscode-textmate';

import {grammars, languages} from '../../generated/textmate/TextMateGrammarManifest';
import {themeState} from '../../theme';
import VSCodeDarkPlusTheme from './VSCodeDarkPlusTheme';
import VSCodeLightPlusTheme from './VSCodeLightPlusTheme';
import {useEffect, useState} from 'react';
import {useRecoilValue} from 'recoil';
import FilepathClassifier from 'shared/textmate-lib/FilepathClassifier';
import createTextMateRegistry from 'shared/textmate-lib/createTextMateRegistry';
import {tokenizeFileContents} from 'shared/textmate-lib/tokenize';
import {loadWASM} from 'vscode-oniguruma';

const URL_TO_ONIG_WASM = '/generated/textmate/onig.wasm';

export type TokenizedParsedDiffHunk = Array<Array<HighlightedToken>>;
export type TokenizedDiffHunk = [before: TokenizedParsedDiffHunk, after: TokenizedParsedDiffHunk];
export type TokenizedDiffHunks = Array<TokenizedDiffHunk> | null;

/**
 * Given a set of hunks from a diff view,
 * asynchronously provide syntax highlighting by tokenizing.
 *
 * Note that we reconstruct the file contents from the diff,
 * so syntax highlighting can be inaccurate since it's missing full context.
 */
export function useTokenizedHunks(path: string, hunks: ParsedDiff['hunks']): TokenizedDiffHunks {
  const theme = useRecoilValue(themeState);

  const [tokenized, setTokenized] = useState<TokenizedDiffHunks>(null);

  useEffect(() => {
    // TODO: run this in a web worker so we don't block the UI.
    tokenizeHunks(theme, path, hunks).then(setTokenized);
  }, [theme, path, hunks]);
  return tokenized;
}

async function tokenizeHunks(
  theme: ThemeColor,
  path: string,
  hunks: Array<{lines: Array<string>}>,
): Promise<TokenizedDiffHunks> {
  const scopeName = getFilepathClassifier().findScopeNameForPath(path);
  if (!scopeName) {
    return null;
  }
  const store = await getGrammerStore(theme);
  const grammar = await store.loadGrammar(scopeName);
  if (grammar == null) {
    return null;
  }
  const tokenizedPatches: TokenizedDiffHunks = hunks
    .map(hunk => recoverFileContentsFromPatchLines(hunk.lines))
    .map(([before, after]) => [
      tokenizeFileContents(before, grammar),
      tokenizeFileContents(after, grammar),
    ]);
  return tokenizedPatches;
}

/**
 * Patch lines start with ' ', '+', or '-'. From this we can reconstruct before & after file contents as strings,
 * which we can actually use in the syntax highlighting.
 */
function recoverFileContentsFromPatchLines(lines: Array<string>): [before: string, after: string] {
  const linesBefore = [];
  const linesAfter = [];
  for (const line of lines) {
    if (line[0] === ' ') {
      linesBefore.push(line.slice(1));
      linesAfter.push(line.slice(1));
    } else if (line[0] === '+') {
      linesAfter.push(line.slice(1));
    } else if (line[0] === '-') {
      linesBefore.push(line.slice(1));
    }
  }

  return [linesBefore.join('\n'), linesAfter.join('\n')];
}

let cachedGrammarStore: {value: Registry; theme: ThemeColor} | null = null;
async function getGrammerStore(theme: ThemeColor) {
  const found = cachedGrammarStore;
  if (found != null && found.theme === theme) {
    return found.value;
  }

  await ensureOnigurumaIsLoaded();
  const themeValues = theme === 'light' ? VSCodeLightPlusTheme : VSCodeDarkPlusTheme;

  const registry = createTextMateRegistry(themeValues, grammars, fetchGrammar);
  cachedGrammarStore = {value: registry, theme};
  return registry;
}

async function fetchGrammar(moduleName: string, type: 'json' | 'plist'): Promise<TextMateGrammar> {
  const uri = `/generated/textmate/${moduleName}.${type}`;
  const response = await fetch(uri);
  const grammar = await response.text();
  return {type, grammar};
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
    };
    await loadWASM(dataOrOptions);
  }
}

let _classifier: FilepathClassifier | null = null;

function getFilepathClassifier(): FilepathClassifier {
  if (_classifier == null) {
    _classifier = new FilepathClassifier(grammars, languages);
  }
  return _classifier;
}
