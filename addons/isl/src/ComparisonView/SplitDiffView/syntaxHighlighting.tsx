/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  SyntaxWorkerRequest,
  SyntaxWorkerResponse,
  TokenizedDiffHunks,
  TokenizedHunk,
} from './syntaxHighlightingTypes';
import type {ParsedDiff} from 'shared/patch/parse';

import {themeState} from '../../theme';
import {registerCleanup, registerDisposable} from '../../utils';
import {SynchronousWorker, WorkerApi} from './workerApi';
import {useAtomValue} from 'jotai';
import {useEffect, useState} from 'react';
import {CancellationToken} from 'shared/CancellationToken';
import {updateTextMateGrammarCSS} from 'shared/textmate-lib/textmateStyles';

// Syntax highlighting is done in a WebWorker. This file contains APIs
// to be called from the main thread, which are delegated to the worker.
// In some environemtns, WebWorker is not available. In that case,
// we fall back to a synchronous worker.

// Useful for testing the non-WebWorker implementation
const forceDisableWorkers = false;

const worker = new WorkerApi<SyntaxWorkerRequest, SyntaxWorkerResponse>(
  window.Worker && !forceDisableWorkers
    ? new Worker(new URL('./syntaxHighlightingWorker', import.meta.url), {type: 'module'})
    : (new SynchronousWorker(() => import('./syntaxHighlightingWorker')) as unknown as Worker),
);
registerDisposable(worker, worker, import.meta.hot);
registerCleanup(
  worker,
  worker.listen('cssColorMap', msg => {
    // During testing-library tear down (ex. syntax highlighting was canceled),
    // `document` may be null. Abort here to avoid errors.
    if (document == null) {
      return undefined;
    }
    updateTextMateGrammarCSS(msg.colorMap);
  }),
  import.meta.hot,
);

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
  const theme = useAtomValue(themeState);

  const [tokenized, setTokenized] = useState<TokenizedDiffHunks | undefined>(undefined);

  useEffect(() => {
    const token = newTrackedCancellationToken();
    worker.request({type: 'tokenizeHunks', theme, path, hunks}, token).then(result => {
      if (!token.isCancelled) {
        setTokenized(result.result);
      }
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
  const theme = useAtomValue(themeState);

  const [tokenized, setTokenized] = useState<TokenizedHunk | undefined>(undefined);

  useEffect(() => {
    if (content == null) {
      return;
    }
    const token = newTrackedCancellationToken();
    worker.request({type: 'tokenizeContents', theme, path, content}, token).then(result => {
      if (!token.isCancelled) {
        setTokenized(result.result);
      }
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
  const theme = useAtomValue(themeState);
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
    const token = newTrackedCancellationToken();

    Promise.all([
      worker.request({type: 'tokenizeContents', theme, path, content: contentBefore}, token),
      worker.request({type: 'tokenizeContents', theme, path, content: contentAfter}, token),
    ]).then(([a, b]) => {
      if (a?.result == null || b?.result == null) {
        return;
      }
      if (!token.isCancelled) {
        setTokenized([a.result, b.result]);
      }
    });
    return () => token.cancel();
  }, [hasBeenVisible, theme, path, contentBefore, contentAfter]);
  return tokenized?.[0].length === contentBefore?.length &&
    tokenized?.[1].length === contentAfter?.length
    ? tokenized
    : undefined;
}

/** Track the `CancellationToken`s so they can be cancelled immediately in tests. */
const cancellationTokens: Set<CancellationToken> = new Set();

/**
 * Cancel all syntax highlighting tasks immediately. This is useful in tests
 * that do not wait for the highlighting to complete and want to avoid the
 * React "act" warning.
 */
export function cancelAllHighlightingTasks() {
  cancellationTokens.forEach(token => token.cancel());
  cancellationTokens.clear();
}

function newTrackedCancellationToken(): CancellationToken {
  const token = new CancellationToken();
  cancellationTokens.add(token);
  token.onCancel(() => cancellationTokens.delete(token));
  return token;
}
