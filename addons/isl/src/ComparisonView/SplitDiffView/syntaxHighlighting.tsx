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
import type {Registry, IGrammar} from 'vscode-textmate';

import {grammars, languages} from '../../generated/textmate/TextMateGrammarManifest';
import {themeState} from '../../theme';
import VSCodeDarkPlusTheme from './VSCodeDarkPlusTheme';
import VSCodeLightPlusTheme from './VSCodeLightPlusTheme';
import {useEffect, useState} from 'react';
import {useRecoilValue} from 'recoil';
import {CancellationToken} from 'shared/CancellationToken';
import FilepathClassifier from 'shared/textmate-lib/FilepathClassifier';
import createTextMateRegistry from 'shared/textmate-lib/createTextMateRegistry';
import {updateTextMateGrammarCSS} from 'shared/textmate-lib/textmateStyles';
import {tokenizeLines} from 'shared/textmate-lib/tokenize';
import {unwrap} from 'shared/utils';
import {loadWASM} from 'vscode-oniguruma';

const URL_TO_ONIG_WASM = 'generated/textmate/onig.wasm';

export type TokenizedHunk = Array<Array<HighlightedToken>>;
export type TokenizedDiffHunk = [before: TokenizedHunk, after: TokenizedHunk];
export type TokenizedDiffHunks = Array<TokenizedDiffHunk>;

/**
 * Given a set of hunks from a diff view,
 * asynchronously provide syntax highlighting by tokenizing.
 *
 * Note that we reconstruct the file contents from the diff,
 * so syntax highlighting can be inaccurate since it's missing full context.
 */
export function useTokenizedHunks(
  path: string,
  hunks: ParsedDiff['hunks'],
): TokenizedDiffHunks | undefined {
  const theme = useRecoilValue(themeState);

  const [tokenized, setTokenized] = useState<TokenizedDiffHunks | undefined>(undefined);

  useEffect(() => {
    const token = new CancellationToken();
    // TODO: run this in a web worker so we don't block the UI?
    // May only be a problem for very large files.
    tokenizeHunks(theme, path, hunks, token).then(result => {
      setTokenized(result);
    });
    return () => token.cancel();
  }, [theme, path, hunks]);
  return tokenized;
}

/**
 * Given a chunk of a file as an array of lines, asynchronously provide syntax highlighting by tokenizing.
 */
export function useTokenizedContents(
  path: string,
  content: Array<string> | undefined,
): TokenizedHunk | undefined {
  const theme = useRecoilValue(themeState);

  const [tokenized, setTokenized] = useState<TokenizedHunk | undefined>(undefined);

  useEffect(() => {
    if (content == null) {
      return;
    }
    const token = new CancellationToken();
    // TODO: run this in a web worker so we don't block the UI?
    // May only be a problem for very large files.
    tokenizeContent(theme, path, content, token).then(result => {
      setTokenized(result);
    });
    return () => token.cancel();
  }, [theme, path, content]);
  return tokenized;
}

/**
 * Given file content of a change before & after, return syntax highlighted versions of those changes.
 * Also takes a parent HTML Element. Sets up an interaction observer to only try syntax highlighting once
 * the container is visible.
 * Note: if parsing contentBefore/After in the caller, it's easy for these to change each render, causing
 * an infinite loop. Memoize contentBefore/contentAfter from the string content in the caller to avoid this.
 */
export function useTokenizedContentsOnceVisible(
  path: string,
  contentBefore: Array<string> | undefined,
  contentAfter: Array<string> | undefined,
  parentNode: React.MutableRefObject<HTMLElement | null>,
): [TokenizedHunk, TokenizedHunk] | undefined {
  const theme = useRecoilValue(themeState);
  const [tokenized, setTokenized] = useState<[TokenizedHunk, TokenizedHunk] | undefined>(undefined);
  const [hasBeenVisible, setHasBeenVisible] = useState(false);

  useEffect(() => {
    if (hasBeenVisible || parentNode.current == null) {
      // no need to start observing again after we've been visible.
      return;
    }
    const observer = new IntersectionObserver((entries, observer) => {
      entries.forEach(entry => {
        if (entry.intersectionRatio > 0) {
          setHasBeenVisible(true);
          // no need to keep observing once we've been visible once and computed the highlights.
          observer.disconnect();
        }
      });
    }, {});
    observer.observe(parentNode.current);
    return () => observer.disconnect();
  }, [parentNode, hasBeenVisible]);

  useEffect(() => {
    if (!hasBeenVisible || contentBefore == null || contentAfter == null) {
      return;
    }
    const token = new CancellationToken();
    Promise.all([
      tokenizeContent(theme, path, contentBefore, token),
      tokenizeContent(theme, path, contentAfter, token),
    ]).then(([a, b]) => {
      if (a == null || b == null) {
        return;
      }
      setTokenized([a, b]);
    });
    return () => token.cancel();
  }, [hasBeenVisible, theme, path, contentBefore, contentAfter]);
  return tokenized?.[0].length === contentBefore?.length &&
    tokenized?.[1].length === contentAfter?.length
    ? tokenized
    : undefined;
}

async function tokenizeHunks(
  theme: ThemeColor,
  path: string,
  hunks: Array<{lines: Array<string>}>,
  cancellationToken: CancellationToken,
): Promise<TokenizedDiffHunks | undefined> {
  await ensureOnigurumaIsLoaded();
  const scopeName = getFilepathClassifier().findScopeNameForPath(path);
  if (!scopeName) {
    return undefined;
  }
  const store = getGrammerStore(theme);
  const grammar = await getGrammar(store, scopeName);
  if (grammar == null) {
    return undefined;
  }
  if (cancellationToken.isCancelled) {
    // check for cancellation before doing expensive highlighting
    return undefined;
  }
  const tokenizedPatches: TokenizedDiffHunks = hunks
    .map(hunk => recoverFileContentsFromPatchLines(hunk.lines))
    .map(([before, after]) => [tokenizeLines(before, grammar), tokenizeLines(after, grammar)]);

  return tokenizedPatches;
}

async function tokenizeContent(
  theme: ThemeColor,
  path: string,
  content: Array<string>,
  cancellationToken: CancellationToken,
): Promise<TokenizedHunk | undefined> {
  await ensureOnigurumaIsLoaded();
  const scopeName = getFilepathClassifier().findScopeNameForPath(path);
  if (!scopeName) {
    return undefined;
  }
  const store = getGrammerStore(theme);
  const grammar = await getGrammar(store, scopeName);
  if (grammar == null) {
    return undefined;
  }
  if (cancellationToken.isCancelled) {
    // check for cancellation before doing expensive highlighting
    return undefined;
  }

  return tokenizeLines(content, grammar);
}

const grammarCache: Map<string, Promise<IGrammar | null>> = new Map();
function getGrammar(store: Registry, scopeName: string): Promise<IGrammar | null> {
  if (grammarCache.has(scopeName)) {
    return unwrap(grammarCache.get(scopeName));
  }
  const grammarPromise = store.loadGrammar(scopeName);
  grammarCache.set(scopeName, grammarPromise);
  return grammarPromise;
}

/**
 * Patch lines start with ' ', '+', or '-'. From this we can reconstruct before & after file contents as strings,
 * which we can actually use in the syntax highlighting.
 */
function recoverFileContentsFromPatchLines(
  lines: Array<string>,
): [before: Array<string>, after: Array<string>] {
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

  return [linesBefore, linesAfter];
}

let cachedGrammarStore: {value: Registry; theme: ThemeColor} | null = null;
function getGrammerStore(theme: ThemeColor) {
  const found = cachedGrammarStore;
  if (found != null && found.theme === theme) {
    return found.value;
  }

  // Grammars were cached according to the store, but the theme may have changed. Just bust the cache
  // to force grammars to reload.
  grammarCache.clear();

  const themeValues = theme === 'light' ? VSCodeLightPlusTheme : VSCodeDarkPlusTheme;

  const registry = createTextMateRegistry(themeValues, grammars, fetchGrammar);

  updateTextMateGrammarCSS(registry.getColorMap());
  cachedGrammarStore = {value: registry, theme};
  return registry;
}

async function fetchGrammar(moduleName: string, type: 'json' | 'plist'): Promise<TextMateGrammar> {
  const uri = `generated/textmate/${moduleName}.${type}`;
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
