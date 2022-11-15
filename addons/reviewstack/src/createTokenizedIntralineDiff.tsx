/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {HighlightedToken} from './textmate/tokenizeFileContents';

import {CSS_CLASS_PREFIX} from './textmate/textmateStyles';
import {diffWordsWithSpace} from 'diff';

/** Type of Chunk: Added, Removed, Unmodified. */
type ChunkType = 'A' | 'R' | 'U';

/**
 * The Myers difference algorithm used by the `diff` Node module is O(ND) where
 * N is the sum of the lengths of the two inputs and D is size of the minimum
 * edit script for the two inputs. As such, large values of N can result in
 * extremely long running times while likely providing little value for the
 * user. For example, a large blob of JSON on a single line compared with the
 * first line of the pretty-printed version (containing only `{`) could be an
 * expensive diff to compute while telling the user nothing of interest.
 *
 * To defend against such pathological cases, we should not bother to compute
 * the intraline diff in certain cases. As an initial heuristic, we impose a
 * threshold for the "maximum input" (i.e., the sum of the lengths of the
 * strings being compared) for which an intraline diff should be computed.
 *
 * Incidentally, tokenization for syntax highlighting has similar issues. At
 * least as of Oct 2022, rather than impose a "max line length," VS Code imposes
 * a "time spent" threshold:
 *
 * https://github.com/microsoft/vscode/blob/504c5a768a001b2099dd2b44e9dc39e10ccdfb56/src/vs/workbench/services/textMate/common/TMTokenization.ts#L39
 *
 * It might be worth considering something similar for intraline diffs.
 */
export const MAX_INPUT_LENGTH_FOR_INTRALINE_DIFF = 300;

/**
 * Normalized version of a `diff.Change` returned by `diffWords()` that is
 * easier to work with when interleaving with syntax higlight information.
 */
type Chunk = {
  type: ChunkType;
  start: number;
  end: number;
};

/**
 * Takes a modified line in the form of `beforeLine` and `afterLine` along with
 * syntax highlighting information that covers each respective line and
 * returns a ReactFragment to display the before/after versions of a line in a
 * side-by-side diff.
 */
export function createTokenizedIntralineDiff(
  beforeLine: string,
  beforeTokens: HighlightedToken[],
  afterLine: string,
  afterTokens: HighlightedToken[],
): [React.ReactFragment | null, React.ReactFragment | null] {
  if (beforeLine.length + afterLine.length > MAX_INPUT_LENGTH_FOR_INTRALINE_DIFF) {
    return [
      applyTokenizationToLine(beforeLine, beforeTokens),
      applyTokenizationToLine(afterLine, afterTokens),
    ];
  }

  const changes = diffWordsWithSpace(beforeLine, afterLine);
  const beforeChunks: Chunk[] = [];
  const afterChunks: Chunk[] = [];
  let beforeLength = 0;
  let afterLength = 0;

  changes.forEach(change => {
    const {added, removed, value} = change;
    const len = value.length;
    if (added) {
      const end = afterLength + len;
      addOrExtend(afterChunks, 'A', afterLength, end);
      afterLength = end;
    } else if (removed) {
      const end = beforeLength + len;
      addOrExtend(beforeChunks, 'R', beforeLength, end);
      beforeLength = end;
    } else {
      const beforeEnd = beforeLength + len;
      addOrExtend(beforeChunks, 'U', beforeLength, beforeEnd);
      beforeLength = beforeEnd;

      const afterEnd = afterLength + len;
      addOrExtend(afterChunks, 'U', afterLength, afterEnd);
      afterLength = afterEnd;
    }
  });

  // Note that the logic in mergeChunksAndTokens() could be done as part of this
  // function to avoid an additional pass over the chunks, but the bookkeeping
  // might get messy.
  return [
    mergeChunksAndTokens(beforeLine, beforeChunks, beforeTokens),
    mergeChunksAndTokens(afterLine, afterChunks, afterTokens),
  ];
}

function addOrExtend(chunks: Chunk[], type: ChunkType, start: number, end: number) {
  const lastEntry = chunks[chunks.length - 1];
  if (lastEntry?.type === type && lastEntry?.end === start) {
    lastEntry.end = end;
  } else {
    chunks.push({type, start, end});
  }
}

/** TODO: Create proper machinery to strip assertions from production builds. */
const ENABLE_ASSERTIONS = false;

type ChunkSpanProps = {
  key: number;
  className: string;
  content: string;
  isChunkStart: boolean;
  isChunkEnd: boolean;
};

function ChunkSpan({
  key,
  className,
  content,
  isChunkStart,
  isChunkEnd,
}: ChunkSpanProps): React.ReactNode {
  let fullClassName = className;
  if (isChunkStart) {
    fullClassName += ' patch-word-begin';
  }
  if (isChunkEnd) {
    fullClassName += ' patch-word-end';
  }
  return (
    <span key={key} className={fullClassName}>
      {content}
    </span>
  );
}

/**
 * Interleave chunks and tokens to produce a properly styled intraline diff.
 */
function mergeChunksAndTokens(
  line: string,
  chunks: Chunk[],
  tokens: HighlightedToken[],
): React.ReactFragment | null {
  if (tokens.length == 0) {
    return null;
  }

  if (ENABLE_ASSERTIONS) {
    // We expect the following invariants to hold, by construction.
    // eslint-disable-next-line no-console
    console.assert(
      chunks.length !== 0,
      'chunks is never empty, even if the line is the empty string',
    );
    // eslint-disable-next-line no-console
    console.assert(
      chunks[chunks.length - 1].end === tokens[tokens.length - 1].end,
      'the final chunk and token must have the same end index to ensure the loop breaks properly',
    );
  }

  const spans: ChunkSpanProps[] = [];
  let chunkIndex = 0;
  let tokenIndex = 0;
  let lastEndIndex = 0;
  let lastChunkType: ChunkType = 'U';
  let isChunkStart = false;
  const maxChunkIndex = chunks.length;
  const maxTokenIndex = tokens.length;
  while (chunkIndex < maxChunkIndex && tokenIndex < maxTokenIndex) {
    const chunk = chunks[chunkIndex];
    const token = tokens[tokenIndex];
    const start = lastEndIndex;
    if (chunk.end === token.end) {
      lastEndIndex = chunk.end;
      ++chunkIndex;
      ++tokenIndex;
    } else if (chunk.end < token.end) {
      lastEndIndex = chunk.end;
      ++chunkIndex;
    } else {
      lastEndIndex = token.end;
      ++tokenIndex;
    }

    const chunkType = chunk.type;
    if (lastChunkType !== 'U' && chunkType !== lastChunkType) {
      spans[spans.length - 1].isChunkEnd = true;
    }
    isChunkStart = chunkType !== 'U' && (chunkType !== lastChunkType || spans.length === 0);
    spans.push(createSpan(line, start, lastEndIndex, token.color, chunkType, isChunkStart));
    lastChunkType = chunkType;
  }

  // Check if last span needs to have isChunkEnd set.
  if (lastChunkType !== 'U') {
    spans[spans.length - 1].isChunkEnd = true;
  }

  return spans.map(ChunkSpan);
}

/**
 * Creates the <span> with the appropriate CSS classes to display a portion of
 * an intraline diff with syntax highlighting.
 */
function createSpan(
  line: string,
  start: number,
  end: number,
  color: number,
  type: ChunkType,
  isChunkStart: boolean,
): ChunkSpanProps {
  let patchClass;
  switch (type) {
    case 'U':
      patchClass = '';
      break;
    case 'A':
      patchClass = ' patch-add-word';
      break;
    case 'R':
      patchClass = ' patch-remove-word';
      break;
  }

  const className = `${CSS_CLASS_PREFIX}${color}${patchClass}`;
  return {
    key: start,
    className,
    content: line.slice(start, end),
    isChunkStart,
    isChunkEnd: false,
  };
}

export function applyTokenizationToLine(
  line: string,
  tokenization: readonly HighlightedToken[],
): React.ReactFragment {
  return tokenization.map(({start, end, color}) => {
    return (
      <span key={start} className={`${CSS_CLASS_PREFIX}${color}`}>
        {line.slice(start, end)}
      </span>
    );
  });
}
