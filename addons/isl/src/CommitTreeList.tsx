/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitTreeWithPreviews} from './getCommitTree';
import type {Hash} from './types';

import {Commit} from './Commit';
import {ErrorNotice} from './ErrorNotice';
import {Icon} from './Icon';
import {pageVisibility} from './codeReview/CodeReviewInfo';
import {t} from './i18n';
import {treeWithPreviews, useMarkOperationsCompleted} from './previews';
import {commitFetchError, latestUncommittedChanges} from './serverAPIState';
import {useRecoilState, useRecoilValue} from 'recoil';

import './CommitTreeList.css';

export function CommitTreeList() {
  // Make sure we trigger subscription to changes to uncommitted changes *before* we have a tree to render,
  // so we don't miss the first returned uncommitted changes mesage.
  // TODO: This is a little ugly, is there a better way to tell recoil to start the subscription immediately?
  // Or should we queue/cache messages?
  useRecoilState(latestUncommittedChanges);
  useRecoilState(pageVisibility);

  useMarkOperationsCompleted();

  const {trees} = useRecoilValue(treeWithPreviews);
  const fetchError = useRecoilValue(commitFetchError);
  return fetchError == null && trees.length === 0 ? (
    <Center>
      <Spinner />
    </Center>
  ) : (
    <>
      {fetchError ? <ErrorNotice title={t('Failed to fetch commits')} error={fetchError} /> : null}
      <div className="commit-tree-root commit-group">
        <MainLineEllipsis />
        {trees.map(tree => createSubtree(tree))}
        <MainLineEllipsis />
      </div>
    </>
  );
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

function Spinner() {
  return (
    <div data-testid="loading-spinner">
      <Icon icon="loading" size="L" />
    </div>
  );
}

function Center({children}: {children: React.ReactNode}) {
  return <div className="center-container">{children}</div>;
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

function MainLineEllipsis() {
  return <div className="commit-ellipsis" />;
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
