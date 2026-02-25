/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ExclusiveOr} from 'shared/typeUtils';
import type {OneIndexedLineNumber} from './types';

import {isLineInSelection} from './useLineRangeSelection';

type Props = {
  beforeLineNumber: number | null;
  before: React.ReactFragment | null;
  afterLineNumber: number | null;
  after: React.ReactFragment | null;
  rowType: SplitDiffRowType;
  path: string;
  unified: boolean;
  openFileToLine?: (lineNumber: OneIndexedLineNumber) => unknown;
  /** Optional callback when a line number is clicked for commenting (in review mode) */
  onCommentClick?: (lineNumber: number, side: 'LEFT' | 'RIGHT', path: string) => void;
  /** Active line range selection for visual highlighting during drag */
  activeLineSelection?: {startLine: number; endLine: number; side: 'LEFT' | 'RIGHT'};
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
  onCommentClick,
  activeLineSelection,
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
      onCommentClick,
      activeLineSelection: activeLineSelection ?? null,
    }),
    <td data-column={unified ? 2 : 1} className={beforeClass}>
      {before}
    </td>,
    LineNumber({
      className: afterClass,
      lineNumber: afterLineNumber,
      path,
      side: 'RIGHT',
      column: unified ? 1 : 2,
      canComment,
      openFileToLine, // opening to a line number only makes sense on the "right" comparison side
      onCommentClick,
      activeLineSelection: activeLineSelection ?? null,
    }),
    <td data-column={unified ? 2 : 3} className={afterClass}>
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
  onCommentClick?: (lineNumber: number, side: 'LEFT' | 'RIGHT', path: string) => void;
  activeLineSelection?: {startLine: number; endLine: number; side: 'LEFT' | 'RIGHT'} | null;
};

function LineNumber({
  className,
  lineNumber,
  path,
  side,
  column,
  canComment,
  openFileToLine,
  onCommentClick,
  activeLineSelection,
}: LineNumberProps): JSX.Element {
  const clickableLineNumber = openFileToLine != null && lineNumber != null;
  const commentable = onCommentClick != null && canComment && lineNumber != null;
  const inSelection = isLineInSelection(lineNumber, side, activeLineSelection ?? null);

  const extraClassName =
    (className != null ? ` ${className}-number` : '') +
    (clickableLineNumber ? ' clickable' : '') +
    (commentable ? ' lineNumber-commentable' : '') +
    (inSelection ? ' lineNumber-in-selection' : '');

  const handleClick = () => {
    if (lineNumber == null) {
      return;
    }
    // Comment click takes priority when in review mode
    if (commentable) {
      onCommentClick(lineNumber, side, path);
    } else if (clickableLineNumber) {
      openFileToLine(lineNumber);
    }
  };

  return (
    <td
      className={`lineNumber${extraClassName} lineNumber-${side}`}
      data-line-number={lineNumber}
      data-path={path}
      data-side={side}
      data-column={column}
      onClick={clickableLineNumber || commentable ? handleClick : undefined}>
      {lineNumber}
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
