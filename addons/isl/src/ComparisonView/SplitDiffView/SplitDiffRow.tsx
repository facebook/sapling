/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ExclusiveOr} from 'shared/typeUtils';
import type {OneIndexedLineNumber} from './types';

type Props = {
  beforeLineNumber: number | null;
  before: React.ReactFragment | null;
  afterLineNumber: number | null;
  after: React.ReactFragment | null;
  rowType: SplitDiffRowType;
  path: string;
  unified: boolean;
  openFileToLine?: (lineNumber: OneIndexedLineNumber) => unknown;
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
};

function LineNumber({
  className,
  lineNumber,
  path,
  side,
  column,
  openFileToLine,
}: LineNumberProps): JSX.Element {
  const clickableLineNumber = openFileToLine != null && lineNumber != null;
  const extraClassName =
    (className != null ? ` ${className}-number` : '') + (clickableLineNumber ? ' clickable' : '');
  return (
    <td
      className={`lineNumber${extraClassName} lineNumber-${side}`}
      data-line-number={lineNumber}
      data-path={path}
      data-side={side}
      data-column={column}
      onClick={clickableLineNumber ? () => openFileToLine(lineNumber) : undefined}>
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
