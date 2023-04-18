/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitTreeWithPreviews} from './getCommitTree';
import type {Hash} from './types';
import type {ContextMenuItem} from 'shared/ContextMenu';

import {BranchIndicator} from './BranchIndicator';
import serverAPI from './ClientToServerAPI';
import {Commit} from './Commit';
import {Center, FlexRow, LargeSpinner} from './ComponentUtils';
import {ErrorNotice} from './ErrorNotice';
import {HighlightCommitsWhileHovering} from './HighlightedCommits';
import {Tooltip, DOCUMENTATION_DELAY} from './Tooltip';
import {allDiffSummaries, codeReviewProvider, pageVisibility} from './codeReview/CodeReviewInfo';
import {T, t} from './i18n';
import {CreateEmptyInitialCommitOperation} from './operations/CreateEmptyInitialCommitOperation';
import {treeWithPreviews, useMarkOperationsCompleted} from './previews';
import {useArrowKeysToChangeSelection} from './selection';
import {
  commitFetchError,
  commitsShownRange,
  isFetchingAdditionalCommits,
  latestUncommittedChangesData,
  useRunOperation,
} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {ErrorShortMessages} from 'isl-server/src/constants';
import {useRecoilState, useRecoilValue} from 'recoil';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {notEmpty} from 'shared/utils';

import './CommitTreeList.css';

export function CommitTreeList() {
  // Make sure we trigger subscription to changes to uncommitted changes *before* we have a tree to render,
  // so we don't miss the first returned uncommitted changes mesage.
  // TODO: This is a little ugly, is there a better way to tell recoil to start the subscription immediately?
  // Or should we queue/cache messages?
  useRecoilState(latestUncommittedChangesData);
  useRecoilState(pageVisibility);

  useMarkOperationsCompleted();

  useArrowKeysToChangeSelection();

  const {trees} = useRecoilValue(treeWithPreviews);
  const fetchError = useRecoilValue(commitFetchError);
  return fetchError == null && trees.length === 0 ? (
    <Center>
      <LargeSpinner />
    </Center>
  ) : (
    <>
      {fetchError ? <CommitFetchError error={fetchError} /> : null}
      <div className="commit-tree-root commit-group" data-testid="commit-tree-root">
        <MainLineEllipsis />
        {trees.map(tree => createSubtree(tree, /* depth */ 0))}
        <MainLineEllipsis>
          <FetchingAdditionalCommitsButton />
          <FetchingAdditionalCommitsIndicator />
        </MainLineEllipsis>
      </div>
    </>
  );
}

function CommitFetchError({error}: {error: Error}) {
  const runOperation = useRunOperation();
  if (error.message === ErrorShortMessages.NoCommitsFetched) {
    return (
      <ErrorNotice
        title={t('No commits found')}
        description={t('If this is a new repository, try adding an initial commit first.')}
        error={error}
        buttons={[
          <VSCodeButton
            appearance="secondary"
            onClick={() => {
              runOperation(new CreateEmptyInitialCommitOperation());
            }}>
            <T>Create empty initial commit</T>
          </VSCodeButton>,
        ]}
      />
    );
  }
  return <ErrorNotice title={t('Failed to fetch commits')} error={error} />;
}

function createSubtree(tree: CommitTreeWithPreviews, depth: number): Array<React.ReactElement> {
  const {info, children, previewType} = tree;
  const isPublic = info.phase === 'public';

  const renderedChildren = (children ?? [])
    .map(tree => createSubtree(tree, depth + 1))
    .map((components, i) => {
      if (!isPublic && i === 0) {
        // first child can be rendered without branching, so single-child lineages render in the same branch
        return components;
      }
      // any additional children render with branches
      return [
        <Branch key={`branch-${info.hash}-${i}`} descendsFrom={info.hash}>
          {components}
        </Branch>,
      ];
    })
    .flat();

  return [
    ...renderedChildren,
    <Commit
      commit={info}
      key={info.hash}
      previewType={previewType}
      hasChildren={renderedChildren.length > 0}
    />,
    depth === 1 ? <StackActions key="stack-actions" tree={tree} /> : null,
  ].filter(notEmpty);
}

function Branch({
  children,
  descendsFrom,
}: {
  children: Array<React.ReactElement>;
  descendsFrom: Hash;
}) {
  return (
    <div className="commit-group" data-testid={`branch-from-${descendsFrom}`}>
      {children}
      <BranchIndicator />
    </div>
  );
}

/**
 * Vertical ellipsis to be rendered on top of the branch line.
 * Expects to rendered as a child of commit-tree-root.
 * Optionally accepts children to render next to the "..."
 */
function MainLineEllipsis({children}: {children?: React.ReactNode}) {
  return (
    <div className="commit-ellipsis">
      <Icon icon="kebab-vertical" />
      <div className="commit-ellipsis-children">{children}</div>
    </div>
  );
}

function FetchingAdditionalCommitsIndicator() {
  const isFetching = useRecoilValue(isFetchingAdditionalCommits);
  return isFetching ? <Icon icon="loading" /> : null;
}

function FetchingAdditionalCommitsButton() {
  const shownRange = useRecoilValue(commitsShownRange);
  const isFetching = useRecoilValue(isFetchingAdditionalCommits);
  if (shownRange === undefined) {
    return null;
  }
  const commitsShownMessage = t('Showing comits from the last $numDays days', {
    replace: {$numDays: shownRange.toString()},
  });
  return (
    <Tooltip placement="top" delayMs={DOCUMENTATION_DELAY} title={commitsShownMessage}>
      <VSCodeButton
        disabled={isFetching}
        onClick={() => {
          serverAPI.postMessage({
            type: 'loadMoreCommits',
          });
        }}
        appearance="icon">
        <T>Load more commits</T>
      </VSCodeButton>
    </Tooltip>
  );
}

function StackActions({tree}: {tree: CommitTreeWithPreviews}): React.ReactElement | null {
  const reviewProvider = useRecoilValue(codeReviewProvider);
  const diffMap = useRecoilValue(allDiffSummaries);
  const runOperation = useRunOperation();

  // buttons at the bottom of the stack
  const actions = [];
  // additional actions hidden behind [...] menu.
  // Non-empty only when actions is non-empty.
  const moreActions: Array<ContextMenuItem> = [];

  const reviewActions =
    diffMap.value == null ? {} : reviewProvider?.getSupportedStackActions(tree, diffMap.value);
  const resubmittableStack = reviewActions?.resubmittableStack;
  const submittableStack = reviewActions?.submittableStack;
  const MIN_STACK_SIZE_TO_SUGGEST_SUBMIT = 2; // don't show "submit stack" on single commits... they're not really "stacks".

  const contextMenu = useContextMenu(() => moreActions);
  if (reviewProvider == null) {
    return null;
  }
  // any existing diffs -> show resubmit stack,
  if (resubmittableStack != null && resubmittableStack.length >= MIN_STACK_SIZE_TO_SUGGEST_SUBMIT) {
    actions.push(
      <HighlightCommitsWhileHovering key="resubmit-stack" toHighlight={resubmittableStack}>
        <VSCodeButton
          appearance="icon"
          onClick={() => {
            runOperation(reviewProvider.submitOperation(resubmittableStack));
          }}>
          <Icon icon="cloud-upload" slot="start" />
          <T>Resubmit stack</T>
        </VSCodeButton>
      </HighlightCommitsWhileHovering>,
    );
    //     any non-submitted diffs -> "submit all commits this stack" in hidden group
    if (
      submittableStack != null &&
      submittableStack.length > 0 &&
      submittableStack.length > resubmittableStack.length
    ) {
      moreActions.push({
        label: (
          <HighlightCommitsWhileHovering key="submit-entire-stack" toHighlight={submittableStack}>
            <FlexRow>
              <Icon icon="cloud-upload" slot="start" />
              <T>Submit entire stack</T>
            </FlexRow>
          </HighlightCommitsWhileHovering>
        ),
        onClick: () => {
          runOperation(
            reviewProvider.submitOperation([...resubmittableStack, ...submittableStack]),
          );
        },
      });
    }
    //     NO non-submitted diffs -> nothing in hidden group
  } else if (
    submittableStack != null &&
    submittableStack.length >= MIN_STACK_SIZE_TO_SUGGEST_SUBMIT
  ) {
    // NO existing diffs -> show submit stack ()
    actions.push(
      <HighlightCommitsWhileHovering key="submit-stack" toHighlight={submittableStack}>
        <VSCodeButton
          appearance="icon"
          onClick={() => {
            runOperation(reviewProvider.submitOperation(submittableStack));
          }}>
          <Icon icon="cloud-upload" slot="start" />
          <T>Submit stack</T>
        </VSCodeButton>
      </HighlightCommitsWhileHovering>,
    );
  }
  if (actions.length === 0) {
    return null;
  }
  const moreActionsButton =
    moreActions.length === 0 ? null : (
      <VSCodeButton key="more-actions" appearance="icon" onClick={contextMenu}>
        <Icon icon="ellipsis" />
      </VSCodeButton>
    );
  return (
    <div className="commit-tree-stack-actions">
      {actions}
      {moreActionsButton}
    </div>
  );
}
