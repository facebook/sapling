/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ThemeColor} from '../../theme';
import type {
  SyntaxWorkerRequest,
  SyntaxWorkerResponse,
  TokenizedDiffHunks,
  TokenizedHunk,
} from './syntaxHighlightingTypes';
import type {IGrammar} from 'vscode-textmate';

import {grammars, languages} from '../../generated/textmate/TextMateGrammarManifest';
import {getGrammerStore, getGrammar} from './grammar';
import {CancellationToken} from 'shared/CancellationToken';
import FilepathClassifier from 'shared/textmate-lib/FilepathClassifier';
import {tokenizeLines} from 'shared/textmate-lib/tokenize';
import {loadWASM} from 'vscode-oniguruma';

const URL_TO_ONIG_WASM = '/generated/textmate/onig.wasm';

/* This file is intended to be executed in a WebWorker, without access to the DOM. */

async function loadGrammar(
  theme: ThemeColor,
  path: string,
  postMessage: (msg: SyntaxWorkerResponse) => void,
): Promise<IGrammar | undefined> {
  await ensureOnigurumaIsLoaded();

  const scopeName = getFilepathClassifier().findScopeNameForPath(path);
  if (!scopeName) {
    return undefined;
  }

  const store = getGrammerStore(theme, colorMap => {
    // tell client the newest colorMap
    postMessage({type: 'cssColorMap', colorMap} as SyntaxWorkerResponse);
  });

  const grammar = await getGrammar(store, scopeName);
  return grammar ?? undefined;
}

export async function handleMessage(
  postMessage: (msg: SyntaxWorkerResponse & {id?: number}) => unknown,
  event: MessageEvent,
) {
  const data = event.data as SyntaxWorkerRequest & {id: number};

  const token = new CancellationToken(); // TODO: remember this and accept "cancel" messages
  switch (data.type) {
    case 'tokenizeContents': {
      const grammar = await loadGrammar(data.theme, data.path, postMessage);
      const result = tokenizeContent(grammar, data.content, token);
      postMessage({type: data.type, id: data.id, result});
      break;
    }
    case 'tokenizeHunks': {
      const grammar = await loadGrammar(data.theme, data.path, postMessage);
      const result = tokenizeHunks(grammar, data.hunks, token);
      postMessage({type: data.type, id: data.id, result});
      break;
    }
  }
}

if (typeof self.document === 'undefined') {
  // inside WebWorker, use global onmessage and postMessage
  onmessage = handleMessage.bind(undefined, postMessage);
  // outside of a WebWorker, the exported `handleMessage` funciton should be used instead.
}

function tokenizeHunks(
  grammar: IGrammar | undefined,
  hunks: Array<{lines: Array<string>}>,
  cancellationToken: CancellationToken,
): TokenizedDiffHunks | undefined {
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

function tokenizeContent(
  grammar: IGrammar | undefined,
  content: Array<string>,
  cancellationToken: CancellationToken,
): TokenizedHunk | undefined {
  if (grammar == null) {
    return undefined;
  }

  if (cancellationToken.isCancelled) {
    // check for cancellation before doing expensive highlighting
    return undefined;
  }

  return tokenizeLines(content, grammar);
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
