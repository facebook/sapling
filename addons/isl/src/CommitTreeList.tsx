/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitTreeWithPreviews} from './getCommitTree';
import type {Hash} from './types';

import serverAPI from './ClientToServerAPI';
import {Commit} from './Commit';
import {Center, LargeSpinner} from './ComponentUtils';
import {ErrorNotice} from './ErrorNotice';
import {Tooltip, DOCUMENTATION_DELAY} from './Tooltip';
import {pageVisibility} from './codeReview/CodeReviewInfo';
import {T, t} from './i18n';
import {CreateEmptyInitialCommitOperation} from './operations/CreateEmptyInitialCommitOperation';
import {treeWithPreviews, useMarkOperationsCompleted} from './previews';
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
import {Icon} from 'shared/Icon';

import './CommitTreeList.css';

export function CommitTreeList() {
  // Make sure we trigger subscription to changes to uncommitted changes *before* we have a tree to render,
  // so we don't miss the first returned uncommitted changes mesage.
  // TODO: This is a little ugly, is there a better way to tell recoil to start the subscription immediately?
  // Or should we queue/cache messages?
  useRecoilState(latestUncommittedChangesData);
  useRecoilState(pageVisibility);

  useMarkOperationsCompleted();

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
        <div className="commit-tree-root commit-group">
          <MainLineEllipsis />
          {trees.map(tree => createSubtree(tree))}
          <MainLineEllipsis>
            <FetchingAdditionalCommitsButton />
            <FetchingAdditionalCommitsIndicator />
          </MainLineEllipsis>
        </div>
      )}
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

function createSubtree(tree: CommitTreeWithPreviews): Array<React.ReactElement> {
  const {info, children, previewType} = tree;
  const isPublic = info.phase === 'public';

  const renderedChildren = (children ?? [])
    .map(tree => createSubtree(tree))
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
  ];
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

const COMPONENT_PADDING = 10;
export const BranchIndicator = () => {
  const width = COMPONENT_PADDING * 2;
  const height = COMPONENT_PADDING * 3;
  // Compensate for line width
  const startX = width + 1;
  const startY = 0;
  const endX = 0;
  const endY = height;
  const verticalLead = height * 0.75;
  const path =
    // start point
    `M${startX} ${startY}` +
    // cubic bezier curve to end point
    `C ${startX} ${startY + verticalLead}, ${endX} ${endY - verticalLead}, ${endX} ${endY}`;
  return (
    <svg
      className="branch-indicator"
      width={width + 2 /* avoid border clipping */}
      height={height}
      xmlns="http://www.w3.org/2000/svg">
      <path d={path} strokeWidth="2px" fill="transparent" />
    </svg>
  );
};

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
