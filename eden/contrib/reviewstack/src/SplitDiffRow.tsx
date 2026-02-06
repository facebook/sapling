/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitHubPullRequestReviewThreadsByLine} from './github/pullRequestTimelineTypes';

import InlineCommentThread from './InlineCommentThread';
import PullRequestNewCommentInput from './PullRequestNewCommentInput';
import {DiffSide} from './generated/graphql';
import {
  gitHubPullRequestCanAddCommentAtom,
  gitHubPullRequestNewCommentInputShownAtom,
} from './jotai';
import {Box} from '@primer/react';
import {useAtomValue} from 'jotai';
import {useMemo} from 'react';
import {notEmpty} from 'shared/utils';

type Props = {
  beforeLineNumber: number | null;
  before: React.ReactFragment | null;
  afterLineNumber: number | null;
  after: React.ReactFragment | null;
  rowType: SplitDiffRowType;
  path: string;
  threads: {
    before: GitHubPullRequestReviewThreadsByLine;
    after: GitHubPullRequestReviewThreadsByLine;
  };
};

type SplitDiffRowType = 'add' | 'common' | 'modify' | 'remove' | 'expanded';

const LINE_NUMBER_STYLE = {
  cursor: 'pointer',
  color: 'fg.subtle',
  ':hover': {color: 'fg.default'},
};

export default function SplitDiffRow({
  beforeLineNumber,
  before,
  afterLineNumber,
  after,
  rowType,
  path,
  threads,
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
        side={DiffSide.Left}
        threads={threads.before}
        canComment={canComment}
      />
      <SplitDiffRowSide
        className={afterClass}
        content={after}
        lineNumber={afterLineNumber}
        path={path}
        side={DiffSide.Right}
        threads={threads.after}
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
  side: DiffSide;
  threads: GitHubPullRequestReviewThreadsByLine;
  canComment: boolean;
};

function SplitDiffRowSide({
  className,
  content,
  lineNumber,
  path,
  side,
  threads,
  canComment,
}: SideProps) {
  const param = useMemo(() => ({path, lineNumber, side}), [path, lineNumber, side]);

  // These atoms are synchronous derived atoms that read from data preloaded
  // by useSplitDiffViewData in <SplitDiffView>.
  const canAddCommentAtom = useMemo(
    () => gitHubPullRequestCanAddCommentAtom(param),
    [param],
  );
  const newCommentInputShownAtom = useMemo(
    () => gitHubPullRequestNewCommentInputShownAtom(param),
    [param],
  );

  const canAddCommentValue = useAtomValue(canAddCommentAtom);
  const isNewCommentInputShownValue = useAtomValue(newCommentInputShownAtom);

  // Only use the values if commenting is allowed for this row
  const canAddComment = canComment && canAddCommentValue;
  const isNewCommentInputShown = canComment && isNewCommentInputShownValue;

  let style;
  let commentThreads = null;
  let input = null;
  if (lineNumber != null) {
    commentThreads = <SplitDiffRowCommentThreads line={lineNumber} threadsByLine={threads} />;

    if (canAddComment) {
      style = LINE_NUMBER_STYLE;
    }

    if (isNewCommentInputShown) {
      input = <PullRequestNewCommentInput line={lineNumber} path={path} side={side} />;
    }
  }

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
        sx={style}
        {...lineNumberBorderStyle}>
        {lineNumber}
      </Box>
      <td className={className}>
        {content}
        {commentThreads}
        {input}
      </td>
    </>
  );
}

function SplitDiffRowCommentThreads({
  line,
  threadsByLine,
}: {
  line: number;
  threadsByLine: GitHubPullRequestReviewThreadsByLine;
}): React.ReactElement | null {
  const threads = threadsByLine.get(line);
  if (threads == null) {
    return null;
  }

  const threadsComments = threads.map(thread => thread.comments.filter(notEmpty));
  return (
    <>
      {threadsComments.map((comments, index) => {
        // Add a prefix to keys for this component to ensure they are distinct
        // from the integer keys returned by createTokenizedIntralineDiff().
        const key = `c-${index}`;
        return <InlineCommentThread key={key} comments={comments} />;
      })}
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
