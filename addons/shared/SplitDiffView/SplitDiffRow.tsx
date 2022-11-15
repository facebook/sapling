/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Box} from '@primer/react';

type Props = {
  beforeLineNumber: number | null;
  before: React.ReactFragment | null;
  afterLineNumber: number | null;
  after: React.ReactFragment | null;
  rowType: SplitDiffRowType;
  path: string;
};

type SplitDiffRowType = 'add' | 'common' | 'modify' | 'remove' | 'expanded';

export default function SplitDiffRow({
  beforeLineNumber,
  before,
  afterLineNumber,
  after,
  rowType,
  path,
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
};

function SplitDiffRowSide({className, content, lineNumber, path, side}: SideProps) {
  const lineNumberBorderStyle = side === 'RIGHT' ? extraRightLineNumberCellProps : {};
  const extraClassName = className != null ? ` ${className}-number` : '';
  return (
    <>
      <Box
        as="td"
        className={`lineNumber${extraClassName}`}
        data-line-number={lineNumber}
        data-path={path}
        data-side={side}
        {...lineNumberBorderStyle}>
        {lineNumber}
      </Box>
      <td className={className}>{content}</td>
    </>
  );
}

const extraRightLineNumberCellProps: {
  borderLeftWidth?: string | undefined;
  borderLeftStyle?: 'solid' | undefined;
  borderLeftColor?: string | undefined;
} = {
  borderLeftWidth: '1px',
  borderLeftStyle: 'solid',
  borderLeftColor: 'border.default',
};
