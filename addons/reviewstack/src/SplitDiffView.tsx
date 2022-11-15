/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './SplitDiffView.css';

import type {LineRangeParams, TokenizedSplitDiff} from './diffServiceWorker';
import type {DiffCommitIDs} from './github/diffTypes';
import type {
  GitHubPullRequestReviewThread,
  GitHubPullRequestReviewThreadsByLine,
} from './github/pullRequestTimelineTypes';
import type {GitObjectID} from './github/types';
import type {NewCommentInputCallbacks} from './recoil';
import type {HighlightedToken} from './textmate/tokenizeFileContents';
import type {Hunk, ParsedDiff} from 'diff';

import SplitDiffRow from './SplitDiffRow';
import {
  applyTokenizationToLine,
  createTokenizedIntralineDiff,
  MAX_INPUT_LENGTH_FOR_INTRALINE_DIFF,
} from './createTokenizedIntralineDiff';
import {diffAndTokenize, lineRange} from './diffServiceClient';
import {DiffSide} from './generated/graphql';
import {
  gitHubPullRequestLineToPositionForFile,
  gitHubPullRequestSelectedVersionIndex,
  gitHubPullRequestVersions,
  gitHubDiffCommitIDs,
  gitHubDiffNewCommentInputCallbacks,
  gitHubThreadsForDiffFile,
  nullAtom,
} from './recoil';
import {findScopeNameForPath} from './textmate/findScopeNameForPath';
import {primerColorMode} from './themeState';
import {groupBy} from './utils';
import {UnfoldIcon} from '@primer/octicons-react';
import {Box, Spinner, Text} from '@primer/react';
import {diffChars} from 'diff';
import React, {useCallback, useEffect, useState} from 'react';
import {useRecoilValue, useRecoilValueLoadable, waitForAll} from 'recoil';
import {FileHeader} from 'shared/SplitDiffView/SplitDiffFileHeader';
import organizeLinesIntoGroups from 'shared/SplitDiffView/organizeLinesIntoGroups';
import {unwrap} from 'shared/utils';

/**
 * It is paramount that the blob for each non-null GitObjectID is written to
 * indexedDB so it can be read by a Web Worker.
 */
export type Props = {
  path: string;
  before: GitObjectID | null;
  after: GitObjectID | null;
  isPullRequest: boolean;
};

/*
 * The Recoil values that are monitored by <SplitDiffView> are unlikely to
 * change over the lifetime of the component whereas `gitHubThreadsForDiffFile`
 * in <SplitDiffViewTable> *is* likely to change as the user adds comments,
 * which is why <SplitDiffViewTable> is its own component.
 */

export default function SplitDiffView({
  path,
  before,
  after,
  isPullRequest,
}: Props): React.ReactElement {
  const scopeName = findScopeNameForPath(path);
  const colorMode = useRecoilValue(primerColorMode);
  const loadable = useRecoilValueLoadable(
    waitForAll([
      diffAndTokenize({path, before, after, scopeName, colorMode}),
      gitHubThreadsForDiffFile(path),
      gitHubDiffNewCommentInputCallbacks,
      // Reset the newCommentInput state when switching views (i.e., with
      // different commit IDs being compared). Although `commitIDs` is not used
      // directly in the effect, we do want to run it when `commitIDs` changes.
      gitHubDiffCommitIDs,

      // TODO(T122242329): This is a bit of a hack where we preload these values
      // to ensure the derived state used by <SplitDiffRowSide> is guaranteed
      // to be available synchronously, avoiding a potentially explosive amount
      // of re-rendering due to notifications from useRecoilValue() about
      // updates to async selectors. The contract between the fetching here and
      // the loading in <SplitDiffRowSide> is very brittle, so ideally this
      // would be redesigned to be more robust.
      isPullRequest ? gitHubPullRequestVersions : nullAtom,
      isPullRequest ? gitHubPullRequestSelectedVersionIndex : nullAtom,
      isPullRequest ? gitHubPullRequestLineToPositionForFile(path) : nullAtom,
    ]),
  );

  if (loadable.state === 'hasValue') {
    const [{patch, tokenization}, allThreads, newCommentInputCallbacks, commitIDs] =
      loadable.contents;
    return (
      <Box borderWidth="1px" borderStyle="solid" borderColor="border.default" borderRadius={2}>
        <FileHeader path={path} />
        <SplitDiffViewTable
          path={path}
          beforeOID={before}
          tokenization={tokenization}
          patch={patch}
          allThreads={allThreads}
          newCommentInputCallbacks={newCommentInputCallbacks}
          commitIDs={commitIDs}
        />
      </Box>
    );
  } else {
    return <div />;
  }
}

const SplitDiffViewTable = React.memo(
  ({
    path,
    beforeOID,
    tokenization,
    patch,
    allThreads,
    newCommentInputCallbacks,
    commitIDs,
  }: {
    path: string;
    beforeOID: GitObjectID | null;
    tokenization: TokenizedSplitDiff;
    patch: ParsedDiff;
    allThreads: {[key in DiffSide]: GitHubPullRequestReviewThread[]} | null;
    newCommentInputCallbacks: NewCommentInputCallbacks | null;
    commitIDs: DiffCommitIDs | null;
  }): React.ReactElement => {
    const {onShowNewCommentInput, onResetNewCommentInput} = newCommentInputCallbacks ?? {};

    useEffect(() => onResetNewCommentInput?.(), [commitIDs, onResetNewCommentInput]);
    const [expandedSeparators, setExpandedSeparators] = useState<Readonly<Set<string>>>(
      () => new Set(),
    );
    const onExpand = useCallback(
      (key: string) => {
        const amendedSet = new Set(expandedSeparators);
        amendedSet.add(key);
        setExpandedSeparators(amendedSet);
      },
      [expandedSeparators, setExpandedSeparators],
    );

    const threads =
      allThreads == null
        ? {
            before: new Map() as GitHubPullRequestReviewThreadsByLine,
            after: new Map() as GitHubPullRequestReviewThreadsByLine,
          }
        : {
            before: threadsByLine(allThreads[DiffSide.Left]),
            after: threadsByLine(allThreads[DiffSide.Right]),
          };
    const {hunks} = patch;
    const lastHunkIndex = hunks.length - 1;
    const rows: React.ReactElement[] = [];
    hunks.forEach((hunk, index) => {
      // Show a separator before the first hunk if the file starts with a
      // section of unmodified lines that is hidden by default.
      if (index === 0 && (hunk.oldStart !== 1 || hunk.newStart !== 1)) {
        // TODO: test empty file that went from 644 to 755?
        const key = 's0';
        if (expandedSeparators.has(key)) {
          const range = {
            oid: unwrap(beforeOID),
            start: 1,
            numLines: hunk.oldStart - 1,
          };
          rows.push(
            <ExpandingSeparator
              key={key}
              range={range}
              path={path}
              beforeLineStart={1}
              afterLineStart={1}
              threads={threads}
              tokenization={tokenization}
            />,
          );
        } else {
          const numLines = Math.max(hunk.oldStart, hunk.newStart) - 1;
          rows.push(<HunkSeparator key={key} numLines={numLines} onExpand={() => onExpand(key)} />);
        }
      }

      addRowsForHunk(hunk, path, threads, tokenization, rows);

      if (index !== lastHunkIndex) {
        const nextHunk = hunks[index + 1];
        const key = `s${hunk.oldStart}`;
        if (expandedSeparators.has(key)) {
          const start = hunk.oldStart + hunk.oldLines;
          const numLines = nextHunk.oldStart - start;
          const range = {
            oid: unwrap(beforeOID),
            start,
            numLines,
          };
          rows.push(
            <ExpandingSeparator
              key={key}
              range={range}
              path={path}
              beforeLineStart={hunk.oldStart + hunk.oldLines}
              afterLineStart={hunk.newStart + hunk.newLines}
              threads={threads}
              tokenization={tokenization}
            />,
          );
        } else {
          const numLines = nextHunk.oldStart - hunk.oldLines - hunk.oldStart;
          rows.push(<HunkSeparator key={key} numLines={numLines} onExpand={() => onExpand(key)} />);
        }
      }
    });

    // Include a final separator if the last hunk does not reach the end of the
    // file. The way things currently work, we do not know the number of lines
    // in the file unless tokenization is non-null. Because we support so many
    // TextMate grammars, this should not be a major issue, in practice, though
    // it should be addressed.
    if (tokenization.before != null) {
      const key = 's-last';
      const lastHunk = hunks[lastHunkIndex];
      if (expandedSeparators.has(key)) {
        const start = lastHunk.oldStart + lastHunk.oldLines;
        const numLines = tokenization.before.length - start;
        const range = {
          oid: unwrap(beforeOID),
          start,
          numLines,
        };
        rows.push(
          <ExpandingSeparator
            key={key}
            range={range}
            path={path}
            beforeLineStart={lastHunk.oldStart + lastHunk.oldLines}
            afterLineStart={lastHunk.newStart + lastHunk.newLines}
            threads={threads}
            tokenization={tokenization}
          />,
        );
      } else {
        const numLines = tokenization.before.length - lastHunk.oldStart - lastHunk.oldLines;
        rows.push(<HunkSeparator key={key} numLines={numLines} onExpand={() => onExpand(key)} />);
      }
    }

    return (
      <table className="SplitDiffView-hunk-table" onClick={onShowNewCommentInput}>
        <colgroup>
          <col width={50} />
          <col width={'50%'} />
          <col width={50} />
          <col width={'50%'} />
        </colgroup>
        <tbody>{rows}</tbody>
      </table>
    );
  },
);

/**
 * Adds new rows to the supplied `rows` array.
 */
function addRowsForHunk(
  hunk: Hunk,
  path: string,
  threads: {
    before: GitHubPullRequestReviewThreadsByLine;
    after: GitHubPullRequestReviewThreadsByLine;
  },
  tokenization: TokenizedSplitDiff,
  rows: React.ReactElement[],
): void {
  const {oldStart, newStart, lines} = hunk;
  const groups = organizeLinesIntoGroups(lines);
  let beforeLineNumber = oldStart;
  let afterLineNumber = newStart;

  const {before: tokenizationBefore, after: tokenizationAfter} = tokenization;
  groups.forEach(group => {
    const {common, removed, added} = group;
    addUnmodifiedRows(
      common,
      path,
      'common',
      beforeLineNumber,
      afterLineNumber,
      threads,
      tokenization,
      rows,
    );
    beforeLineNumber += common.length;
    afterLineNumber += common.length;

    const maxIndex = Math.max(removed.length, added.length);
    for (let index = 0; index < maxIndex; ++index) {
      const removedLine = removed[index];
      const addedLine = added[index];
      if (removedLine != null && addedLine != null) {
        let beforeAndAfter;
        if (tokenizationBefore != null && tokenizationAfter != null) {
          beforeAndAfter = createTokenizedIntralineDiff(
            removedLine,
            tokenizationBefore[beforeLineNumber - 1],
            addedLine,
            tokenizationAfter[afterLineNumber - 1],
          );
        } else {
          beforeAndAfter = createIntralineDiff(removedLine, addedLine);
        }
        const [before, after] = beforeAndAfter;
        rows.push(
          <SplitDiffRow
            key={`${beforeLineNumber}/${afterLineNumber}`}
            beforeLineNumber={beforeLineNumber}
            before={before}
            afterLineNumber={afterLineNumber}
            after={after}
            rowType="modify"
            path={path}
            threads={threads}
          />,
        );
        ++beforeLineNumber;
        ++afterLineNumber;
      } else if (removedLine != null) {
        rows.push(
          <SplitDiffRow
            key={`${beforeLineNumber}/`}
            beforeLineNumber={beforeLineNumber}
            before={
              tokenizationBefore != null
                ? applyTokenization(removedLine, beforeLineNumber, tokenizationBefore)
                : removedLine
            }
            afterLineNumber={null}
            after={null}
            rowType="remove"
            path={path}
            threads={threads}
          />,
        );
        ++beforeLineNumber;
      } else {
        rows.push(
          <SplitDiffRow
            key={`/${afterLineNumber}`}
            beforeLineNumber={null}
            before={null}
            afterLineNumber={afterLineNumber}
            after={
              tokenizationAfter != null
                ? applyTokenization(addedLine, afterLineNumber, tokenizationAfter)
                : addedLine
            }
            rowType="add"
            path={path}
            threads={threads}
          />,
        );
        ++afterLineNumber;
      }
    }
  });
}

/**
 * Adds new rows to the supplied `rows` array.
 */
function addUnmodifiedRows(
  lines: string[],
  path: string,
  rowType: 'common' | 'expanded',
  initialBeforeLineNumber: number,
  initialAfterLineNumber: number,
  threads: {
    before: GitHubPullRequestReviewThreadsByLine;
    after: GitHubPullRequestReviewThreadsByLine;
  },
  tokenization: TokenizedSplitDiff,
  rows: React.ReactElement[],
): void {
  let beforeLineNumber = initialBeforeLineNumber;
  let afterLineNumber = initialAfterLineNumber;
  const {before: tokenizationBefore, after: tokenizationAfter} = tokenization;
  lines.forEach(lineContent => {
    rows.push(
      <SplitDiffRow
        key={`${beforeLineNumber}/${afterLineNumber}`}
        beforeLineNumber={beforeLineNumber}
        before={
          tokenizationBefore != null
            ? applyTokenization(lineContent, beforeLineNumber, tokenizationBefore)
            : lineContent
        }
        afterLineNumber={afterLineNumber}
        after={
          tokenizationAfter != null
            ? applyTokenization(lineContent, afterLineNumber, tokenizationAfter)
            : lineContent
        }
        rowType={rowType}
        path={path}
        threads={threads}
      />,
    );
    ++beforeLineNumber;
    ++afterLineNumber;
  });
}

function createIntralineDiff(
  before: string,
  after: string,
): [React.ReactFragment, React.ReactFragment] {
  // For lines longer than this, diffChars() can get very expensive to compute
  // and is likely of little value to the user.
  if (before.length + after.length > MAX_INPUT_LENGTH_FOR_INTRALINE_DIFF) {
    return [before, after];
  }

  const changes = diffChars(before, after);
  const beforeElements: React.ReactNode[] = [];
  const afterElements: React.ReactNode[] = [];
  changes.forEach((change, index) => {
    const {added, removed, value} = change;
    if (added) {
      afterElements.push(
        <span key={index} className="patch-add-word">
          {value}
        </span>,
      );
    } else if (removed) {
      beforeElements.push(
        <span key={index} className="patch-remove-word">
          {value}
        </span>,
      );
    } else {
      beforeElements.push(value);
      afterElements.push(value);
    }
  });

  return [beforeElements, afterElements];
}

function threadsByLine(
  threads: GitHubPullRequestReviewThread[],
): GitHubPullRequestReviewThreadsByLine {
  return groupBy(threads, thread => thread.originalLine ?? null);
}

/**
 * Visual element to delimit the discontinuity in a SplitDiffView.
 */
function HunkSeparator({
  numLines,
  onExpand,
}: {
  numLines: number;
  onExpand: () => unknown;
}): React.ReactElement {
  // TODO: Ensure numLines is never below a certain threshold: it takes up more
  // space to display the separator than it does to display the text (though
  // admittedly fetching the collapsed text is an async operation).
  const label = numLines === 1 ? 'Expand 1 line' : `Expand ${numLines} lines`;
  return (
    <SeparatorRow>
      <Box
        display="inline-block"
        onClick={onExpand}
        padding={1}
        sx={{cursor: 'pointer', ':hover': {bg: 'accent.emphasis', color: 'fg.onEmphasis'}}}>
        <UnfoldIcon size={16} />
        <Text paddingX={4}>{label}</Text>
        <UnfoldIcon size={16} />
      </Box>
    </SeparatorRow>
  );
}

type ExpandingSeparatorProps = {
  path: string;
  range: LineRangeParams;
  beforeLineStart: number;
  afterLineStart: number;
  threads: {
    before: GitHubPullRequestReviewThreadsByLine;
    after: GitHubPullRequestReviewThreadsByLine;
  };
  tokenization: TokenizedSplitDiff;
};

/**
 * This replaces a <HunkSeparator> when the user clicks on it to expand the
 * hidden file contents.
 */
function ExpandingSeparator({
  path,
  range,
  beforeLineStart,
  afterLineStart,
  threads,
  tokenization,
}: ExpandingSeparatorProps): React.ReactElement {
  const loadable = useRecoilValueLoadable(lineRange(range));
  switch (loadable.state) {
    case 'hasValue': {
      const rows: React.ReactElement[] = [];
      const lines = loadable.contents;
      addUnmodifiedRows(
        lines,
        path,
        'expanded',
        beforeLineStart,
        afterLineStart,
        threads,
        tokenization,
        rows,
      );
      return <>{rows}</>;
    }
    case 'loading': {
      return (
        <SeparatorRow>
          <Box
            padding={1}
            display="flex"
            flexDirection="row"
            justifyContent="center"
            alignItems="center">
            <Box display="flex" alignItems="center">
              <Spinner size="small" />
              <Text marginLeft={2}>Loading...</Text>
            </Box>
          </Box>
        </SeparatorRow>
      );
    }
    case 'hasError': {
      return (
        <SeparatorRow>
          <Box
            padding={1}
            display="flex"
            flexDirection="row"
            justifyContent="center"
            alignItems="center">
            <Box display="flex" alignItems="center">
              <Text>Error: {loadable.contents.message}</Text>
            </Box>
          </Box>
        </SeparatorRow>
      );
    }
  }
}

function SeparatorRow({children}: {children: React.ReactNode}): React.ReactElement {
  return (
    <Box as="tr" bg="accent.subtle" color="fg.muted" height={12}>
      <Box as="td" colSpan={4} className="separator">
        {children}
      </Box>
    </Box>
  );
}

/**
 * Simple tokenization case when there is no styling due to an intraline diff.
 */
function applyTokenization(
  line: string,
  lineNumber: number,
  tokenization: readonly HighlightedToken[][],
): React.ReactFragment {
  const info = tokenization[lineNumber - 1];
  if (info != null && info.length !== 0) {
    return applyTokenizationToLine(line, info);
  } else {
    return line;
  }
}
