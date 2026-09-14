/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {JSX, MouseEvent as ReactMouseEvent} from 'react';

import type {ExclusiveOr} from 'shared/typeUtils';
import type {DiffLineLocation, OneIndexedLineNumber} from './types';

type Props = {
  beforeLineNumber: number | null;
  before: React.ReactNode;
  afterLineNumber: number | null;
  after: React.ReactNode;
  rowType: SplitDiffRowType;
  path: string;
  unified: boolean;
  openFileToLine?: (lineNumber: OneIndexedLineNumber) => unknown;
  commentSelection?: DiffCommentSelectionController;
};

export type DiffCommentSelectionController = {
  isSelected: (location: DiffLineLocation) => boolean;
  isCommented: (location: DiffLineLocation) => boolean;
  begin: (location: DiffLineLocation, event: ReactMouseEvent<HTMLTableCellElement>) => void;
  extend: (location: DiffLineLocation, event: ReactMouseEvent<HTMLTableCellElement>) => void;
  finish: () => void;
  startComment: (location: DiffLineLocation) => void;
};

type SplitDiffRowType = 'add' | 'common' | 'modify' | 'remove' | 'expanded';

export default function SplitDiffRow({
  beforeLineNumber,
  before,
  afterLineNumber,
  after,
  rowType,
  path,
  unified,
  openFileToLine,
  commentSelection,
}: Props): [JSX.Element, JSX.Element, JSX.Element, JSX.Element] {
  let beforeClass;
  let afterClass;
  switch (rowType) {
    case 'remove':
      beforeClass = 'patch-remove-line';
      afterClass = undefined;
      break;
    case 'modify':
      beforeClass = 'patch-remove-line';
      afterClass = 'patch-add-line';
      break;
    case 'add':
      beforeClass = undefined;
      afterClass = 'patch-add-line';
      break;
    case 'common':
      beforeClass = undefined;
      afterClass = undefined;
      break;
    case 'expanded':
      beforeClass = 'patch-expanded';
      afterClass = 'patch-expanded';
      break;
  }

  // Note that 'expanded' is a special case of 'common' where it is code that is
  // common to both sides of the diff, but was previously displayed as
  // collapsed. For whatever reason, GitHub does not make it possible to comment
  // on lines outside of the patch contents in PRs:
  //
  // https://github.com/isaacs/github/issues/1655
  //
  // Even if you try to do so programmatically via the GraphQL API, it *still*
  // doesn't work, so this seems to be some quirk in the underlying data model.
  const canComment = rowType !== 'expanded';

  return [
    LineNumber({
      className: beforeClass,
      lineNumber: beforeLineNumber,
      path,
      side: 'LEFT',
      column: 0,
      canComment,
      commentSelection,
    }),
    <td key="before" data-column={unified ? 2 : 1} className={beforeClass}>
      {before}
    </td>,
    LineNumber({
      className: afterClass,
      lineNumber: afterLineNumber,
      path,
      side: 'RIGHT',
      column: unified ? 1 : 2,
      canComment,
      commentSelection,
      openFileToLine, // opening to a line number only makes sense on the "right" comparison side
    }),
    <td key="after" data-column={unified ? 2 : 3} className={afterClass}>
      {after}
    </td>,
  ];
}

type LineNumberProps = {
  className?: string;
  lineNumber: number | null;
  path: string;
  side: 'LEFT' | 'RIGHT';
  column: number;
  canComment: boolean;
  openFileToLine?: (lineNumber: OneIndexedLineNumber) => unknown;
  commentSelection?: DiffCommentSelectionController;
};

function LineNumber({
  className,
  lineNumber,
  path,
  side,
  column,
  canComment,
  openFileToLine,
  commentSelection,
}: LineNumberProps): JSX.Element {
  const clickableLineNumber = openFileToLine != null && lineNumber != null;
  const location =
    lineNumber == null ? undefined : {path, line: lineNumber as OneIndexedLineNumber, side};
  const selected = location != null && commentSelection?.isSelected(location) === true;
  const commented = location != null && commentSelection?.isCommented(location) === true;
  const extraClassName =
    (className != null ? ` ${className}-number` : '') +
    (clickableLineNumber ? ' clickable' : '') +
    (commented ? ' split-diff-review-commented-line' : '') +
    (selected ? ' split-diff-review-selected-line' : '');
  return (
    <td
      className={`lineNumber${extraClassName} lineNumber-${side}`}
      data-line-number={lineNumber}
      data-path={path}
      data-side={side}
      data-column={column}
      aria-selected={selected || undefined}
      onMouseDown={
        canComment && location != null && commentSelection != null
          ? event => commentSelection.begin(location, event)
          : undefined
      }
      onMouseEnter={
        canComment && location != null && commentSelection != null
          ? event => commentSelection.extend(location, event)
          : undefined
      }
      onMouseUp={commentSelection?.finish}
      onClick={clickableLineNumber ? () => openFileToLine(lineNumber) : undefined}>
      <span>{lineNumber}</span>
      {canComment && location != null && commentSelection != null && (
        <button
          className="split-diff-review-add-button"
          aria-label={`Add comment on ${path}:${lineNumber}`}
          onMouseDown={event => event.stopPropagation()}
          onClick={event => {
            event.stopPropagation();
            commentSelection.startComment(location);
          }}>
          +
        </button>
      )}
    </td>
  );
}

export function BlankLineNumber({before}: ExclusiveOr<{before: true}, {after: true}>) {
  return (
    <td
      className={
        before
          ? 'patch-remove-line-number lineNumber lineNumber-LEFT'
          : 'patch-add-line-number lineNumber lineNumber-RIGHT'
      }
    />
  );
}
