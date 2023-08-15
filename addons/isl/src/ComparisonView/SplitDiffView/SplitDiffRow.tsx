/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {OneIndexedLineNumber} from './types';

type Props = {
  beforeLineNumber: number | null;
  before: React.ReactFragment | null;
  afterLineNumber: number | null;
  after: React.ReactFragment | null;
  rowType: SplitDiffRowType;
  path: string;
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
  openFileToLine,
}: Props): React.ReactElement {
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

  return (
    <tr>
      <SplitDiffRowSide
        className={beforeClass}
        content={before}
        lineNumber={beforeLineNumber}
        path={path}
        side={'LEFT'}
        canComment={canComment}
      />
      <SplitDiffRowSide
        className={afterClass}
        content={after}
        lineNumber={afterLineNumber}
        path={path}
        side={'RIGHT'}
        canComment={canComment}
        openFileToLine={openFileToLine} // opening to a line number only makes sense on the "right" comparison side
      />
    </tr>
  );
}

type SideProps = {
  className?: string;
  content: React.ReactFragment | null;
  lineNumber: number | null;
  path: string;
  side: 'LEFT' | 'RIGHT';
  canComment: boolean;
  openFileToLine?: (lineNumber: OneIndexedLineNumber) => unknown;
};

function SplitDiffRowSide({className, content, lineNumber, path, side, openFileToLine}: SideProps) {
  const clickableLineNumber = openFileToLine != null && lineNumber != null;
  const extraClassName =
    (className != null ? ` ${className}-number` : '') + (clickableLineNumber ? ' clickable' : '');
  return (
    <>
      <td
        className={`lineNumber${extraClassName} lineNumber-${side}`}
        data-line-number={lineNumber}
        data-path={path}
        data-side={side}
        data-column={side === 'LEFT' ? '0' : '2'}
        onClick={clickableLineNumber ? () => openFileToLine(lineNumber) : undefined}>
        {lineNumber}
      </td>
      <td data-column={side === 'LEFT' ? '1' : '3'} className={className}>
        {content}
      </td>
    </>
  );
}
