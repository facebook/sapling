/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitTree, CommitTreeWithPreviews} from './getCommitTree';
import type {Hash} from './types';

import {BranchIndicator} from './BranchIndicator';
import serverAPI from './ClientToServerAPI';
import {Commit} from './Commit';
import {Center, LargeSpinner} from './ComponentUtils';
import {ErrorNotice} from './ErrorNotice';
import {StackActions} from './StackActions';
import {Tooltip, DOCUMENTATION_DELAY} from './Tooltip';
import {pageVisibility} from './codeReview/CodeReviewInfo';
import {T, t} from './i18n';
import {CreateEmptyInitialCommitOperation} from './operations/CreateEmptyInitialCommitOperation';
import {treeWithPreviews, useMarkOperationsCompleted} from './previews';
import {isNarrowCommitTree} from './responsive';
import {useArrowKeysToChangeSelection, useBackspaceToHideSelected} from './selection';
import {
  commitFetchError,
  commitsShownRange,
  isFetchingAdditionalCommits,
  latestUncommittedChangesData,
  useRunOperation,
} from './serverAPIState';
import {MaybeEditStackModal} from './stackEdit/ui/EditStackModal';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {ErrorShortMessages} from 'isl-server/src/constants';
import {useRecoilState, useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';
import {notEmpty} from 'shared/utils';

import './CommitTreeList.css';

export function CommitTreeList() {
  // Make sure we trigger subscription to changes to uncommitted changes *before* we have a tree to render,
  // so we don't miss the first returned uncommitted changes message.
  // TODO: This is a little ugly, is there a better way to tell recoil to start the subscription immediately?
  // Or should we queue/cache messages?
  useRecoilState(latestUncommittedChangesData);
  useRecoilState(pageVisibility);

  useMarkOperationsCompleted();

  useArrowKeysToChangeSelection();
  useBackspaceToHideSelected();

  const isNarrow = useRecoilValue(isNarrowCommitTree);

  const {trees} = useRecoilValue(treeWithPreviews);
  const fetchError = useRecoilValue(commitFetchError);
  return fetchError == null && trees.length === 0 ? (
    <Center>
      <LargeSpinner />
    </Center>
  ) : (
    <>
      {fetchError ? <CommitFetchError error={fetchError} /> : null}
      {trees.length === 0 ? null : (
        <div
          className={
            'commit-tree-root commit-group with-vertical-line' +
            (isNarrow ? ' commit-tree-narrow' : '')
          }
          data-testid="commit-tree-root">
          <MainLineEllipsis />
          {trees.filter(shouldShowPublicCommit).map(tree => (
            <SubTree key={tree.info.hash} tree={tree} depth={0} />
          ))}
          <MainLineEllipsis>
            <FetchingAdditionalCommitsButton />
            <FetchingAdditionalCommitsIndicator />
          </MainLineEllipsis>
          <MaybeEditStackModal />
        </div>
      )}
    </>
  );
}

/**
 * Ensure only relevant public commits are shown.
 * `sl log` does this kind of filtering for us anyway, but
 * if a commit is hidden due to previews or optimistic state,
 * we can violate these conditions.
 */
function shouldShowPublicCommit(tree: CommitTree) {
  return (
    tree.info.isHead ||
    tree.children.length > 0 ||
    tree.info.bookmarks.length > 0 ||
    tree.info.remoteBookmarks.length > 0 ||
    (tree.info.stableCommitMetadata?.length ?? 0) > 0
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

function SubTree({tree, depth}: {tree: CommitTreeWithPreviews; depth: number}): React.ReactElement {
  const {info, children, previewType} = tree;
  const isPublic = info.phase === 'public';

  const stackActions =
    !isPublic && depth === 1 ? <StackActions key="stack-actions" tree={tree} /> : null;

  const renderedChildren = (children ?? [])
    .map(tree => <SubTree key={`tree-${tree.info.hash}`} tree={tree} depth={depth + 1} />)
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

  const rendered = [
    ...renderedChildren,
    <Commit
      commit={info}
      key={info.hash}
      previewType={previewType}
      hasChildren={renderedChildren.length > 0}
    />,
    stackActions,
  ].filter(notEmpty);

  return <>{rendered}</>;
}

function Branch({
  children,
  descendsFrom,
  className,
}: {
  children: React.ReactElement;
  descendsFrom: Hash;
  className?: string;
}) {
  return (
    <div
      className={`commit-group ${className ?? 'with-vertical-line'}`}
      data-testid={`branch-from-${descendsFrom}`}>
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
