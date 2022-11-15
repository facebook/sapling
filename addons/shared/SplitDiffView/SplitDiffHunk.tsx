/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Context, LineRangeParams} from './types';
import type {Hunk, ParsedDiff} from 'diff';

import SplitDiffRow from './SplitDiffRow';
import organizeLinesIntoGroups from './organizeLinesIntoGroups';
import {UnfoldIcon} from '@primer/octicons-react';
import {Box, Spinner, Text} from '@primer/react';
import {diffChars} from 'diff';
import React, {useCallback, useState} from 'react';
import {useRecoilValueLoadable} from 'recoil';

const MAX_INPUT_LENGTH_FOR_INTRALINE_DIFF = 300;

export type SplitDiffTableProps<Id> = {
  ctx: Context<Id>;
  path: string;
  patch: ParsedDiff;
  preamble?: Array<React.ReactElement>;
};

export const SplitDiffTable = React.memo(
  <Id,>({ctx, path, patch, preamble}: SplitDiffTableProps<Id>): React.ReactElement => {
    const [deletedFileExpanded, setDeletedFileExpanded] = useState<boolean>(false);
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

    const t = ctx.translate ?? (s => s);

    const isDeleted = patch.newFileName === '/dev/null';

    const {hunks} = patch;
    const lastHunkIndex = hunks.length - 1;
    const rows: React.ReactElement[] = [...(preamble ?? [])];
    if (!isDeleted || deletedFileExpanded) {
      hunks.forEach((hunk, index) => {
        // Show a separator before the first hunk if the file starts with a
        // section of unmodified lines that is hidden by default.
        if (index === 0 && (hunk.oldStart !== 1 || hunk.newStart !== 1)) {
          // TODO: test empty file that went from 644 to 755?
          const key = 's0';
          if (expandedSeparators.has(key)) {
            const range: LineRangeParams<Id> = {
              id: ctx.id,
              start: 1,
              numLines: hunk.oldStart - 1,
            };
            rows.push(
              <ExpandingSeparator
                key={key}
                ctx={ctx}
                range={range}
                path={path}
                beforeLineStart={1}
                afterLineStart={1}
                t={t}
              />,
            );
          } else {
            const numLines = Math.max(hunk.oldStart, hunk.newStart) - 1;
            rows.push(
              <HunkSeparator key={key} numLines={numLines} onExpand={() => onExpand(key)} t={t} />,
            );
          }
        }

        addRowsForHunk(hunk, path, rows);

        if (index !== lastHunkIndex) {
          const nextHunk = hunks[index + 1];
          const key = `s${hunk.oldStart}`;
          if (expandedSeparators.has(key)) {
            const start = hunk.oldStart + hunk.oldLines;
            const numLines = nextHunk.oldStart - start;
            const range: LineRangeParams<Id> = {
              id: ctx.id,
              start,
              numLines,
            };
            rows.push(
              <ExpandingSeparator
                key={key}
                ctx={ctx}
                range={range}
                path={path}
                beforeLineStart={hunk.oldStart + hunk.oldLines}
                afterLineStart={hunk.newStart + hunk.newLines}
                t={t}
              />,
            );
          } else {
            const numLines = nextHunk.oldStart - hunk.oldLines - hunk.oldStart;
            rows.push(
              <HunkSeparator key={key} numLines={numLines} onExpand={() => onExpand(key)} t={t} />,
            );
          }
        }
      });
    } else {
      rows.push(
        <SeparatorRow key={'show-deleted'}>
          <Box
            display="inline-block"
            onClick={() => setDeletedFileExpanded(true)}
            padding={1}
            sx={{cursor: 'pointer', ':hover': {bg: 'accent.emphasis', color: 'fg.onEmphasis'}}}>
            <UnfoldIcon size={16} />
            <Text paddingX={4}>{t('Show deleted file')}</Text>
            <UnfoldIcon size={16} />
          </Box>
        </SeparatorRow>,
      );
    }

    return (
      <table className="SplitDiffView-hunk-table">
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
function addRowsForHunk(hunk: Hunk, path: string, rows: React.ReactElement[]): void {
  const {oldStart, newStart, lines} = hunk;
  const groups = organizeLinesIntoGroups(lines);
  let beforeLineNumber = oldStart;
  let afterLineNumber = newStart;

  groups.forEach(group => {
    const {common, removed, added} = group;
    addUnmodifiedRows(common, path, 'common', beforeLineNumber, afterLineNumber, rows);
    beforeLineNumber += common.length;
    afterLineNumber += common.length;

    const maxIndex = Math.max(removed.length, added.length);
    for (let index = 0; index < maxIndex; ++index) {
      const removedLine = removed[index];
      const addedLine = added[index];
      if (removedLine != null && addedLine != null) {
        const beforeAndAfter = createIntralineDiff(removedLine, addedLine);
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
          />,
        );
        ++beforeLineNumber;
        ++afterLineNumber;
      } else if (removedLine != null) {
        rows.push(
          <SplitDiffRow
            key={`${beforeLineNumber}/`}
            beforeLineNumber={beforeLineNumber}
            before={removedLine}
            afterLineNumber={null}
            after={null}
            rowType="remove"
            path={path}
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
            after={addedLine}
            rowType="add"
            path={path}
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
  rows: React.ReactElement[],
): void {
  let beforeLineNumber = initialBeforeLineNumber;
  let afterLineNumber = initialAfterLineNumber;
  lines.forEach(lineContent => {
    rows.push(
      <SplitDiffRow
        key={`${beforeLineNumber}/${afterLineNumber}`}
        beforeLineNumber={beforeLineNumber}
        before={lineContent}
        afterLineNumber={afterLineNumber}
        after={lineContent}
        rowType={rowType}
        path={path}
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

/**
 * Visual element to delimit the discontinuity in a SplitDiffView.
 */
function HunkSeparator({
  numLines,
  onExpand,
  t,
}: {
  numLines: number;
  onExpand: () => unknown;
  t: (s: string) => string;
}): React.ReactElement | null {
  if (numLines === 0) {
    return null;
  }
  // TODO: Ensure numLines is never below a certain threshold: it takes up more
  // space to display the separator than it does to display the text (though
  // admittedly fetching the collapsed text is an async operation).
  const label = numLines === 1 ? t('Expand 1 line') : t(`Expand ${numLines} lines`);
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

type ExpandingSeparatorProps<Id> = {
  ctx: Context<Id>;
  path: string;
  range: LineRangeParams<Id>;
  beforeLineStart: number;
  afterLineStart: number;
  t: (s: string) => string;
};

/**
 * This replaces a <HunkSeparator> when the user clicks on it to expand the
 * hidden file contents.
 */
function ExpandingSeparator<Id>({
  ctx,
  path,
  range,
  beforeLineStart,
  afterLineStart,
  t,
}: ExpandingSeparatorProps<Id>): React.ReactElement {
  const loadable = useRecoilValueLoadable(ctx.atoms.lineRange(range));
  switch (loadable.state) {
    case 'hasValue': {
      const rows: React.ReactElement[] = [];
      const lines = loadable.contents;
      addUnmodifiedRows(lines, path, 'expanded', beforeLineStart, afterLineStart, rows);
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
              <Text marginLeft={2}>{t('Loading...')}</Text>
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
              <Text>
                {t('Error:')} {loadable.contents.message}
              </Text>
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
