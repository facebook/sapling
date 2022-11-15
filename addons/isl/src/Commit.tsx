/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, SuccessorInfo} from './types';

import {hasUnsavedEditedCommitMessage} from './CommitInfo';
import {BranchIndicator} from './CommitTreeList';
import {Icon} from './Icon';
import {Tooltip} from './Tooltip';
import {UncommitButton} from './UncommitButton';
import {UncommittedChanges} from './UncommittedChanges';
import {DiffInfo} from './codeReview/DiffBadge';
import {isDescendant} from './getCommitTree';
import {t, T} from './i18n';
import {GotoOperation} from './operations/GotoOperation';
import {RebaseOperation} from './operations/RebaseOperation';
import {CommitPreview, operationBeingPreviewed} from './previews';
import {RelativeDate} from './relativeDate';
import {useCommitSelection} from './selection';
import {
  isFetchingUncommittedChanges,
  latestCommitTreeMap,
  latestUncommittedChanges,
  useRunOperation,
  useRunPreviewedOperation,
} from './serverAPIState';
import {VSCodeButton, VSCodeTag} from '@vscode/webview-ui-toolkit/react';
import React, {memo} from 'react';
import {useRecoilCallback, useRecoilValue} from 'recoil';

function isDraggablePreview(previewType?: CommitPreview): boolean {
  switch (previewType) {
    // dragging preview descendants would be confusing (it would reset part of your drag),
    // you probably meant to drag the root.
    case CommitPreview.REBASE_DESCENDANT:
    // old commits are already being dragged
    case CommitPreview.REBASE_OLD:
      return false;

    // you CAN let go of the preview and drag it again
    case CommitPreview.REBASE_ROOT:
    // optimistic rebase commits act like normal, they can be dragged just fine
    case CommitPreview.REBASE_OPTIMISTIC_DESCENDANT:
    case CommitPreview.REBASE_OPTIMISTIC_ROOT:
    case undefined:
    // other unrelated previews are draggable
    default:
      return true;
  }
}

/**
 * Some preview types should not allow actions on top of them
 * For example, you can't click goto on the preview of dragging a rebase,
 * but you can goto on the optimistic form of a running rebase.
 */
function previewPreventsActions(preview?: CommitPreview): boolean {
  switch (preview) {
    case CommitPreview.REBASE_OLD:
    case CommitPreview.REBASE_DESCENDANT:
    case CommitPreview.REBASE_ROOT:
      return true;
  }
  return false;
}

export const Commit = memo(
  ({
    commit,
    previewType,
    hasChildren,
  }: {
    commit: CommitInfo;
    previewType?: CommitPreview;
    hasChildren: boolean;
  }) => {
    const isPublic = commit.phase === 'public';

    const handlePreviewedOperation = useRunPreviewedOperation();
    const runOperation = useRunOperation();

    const {isSelected, onClickToSelect} = useCommitSelection(commit.hash);
    const actionsPrevented = previewPreventsActions(previewType);

    return (
      <div
        className={
          'commit' +
          (commit.isHead ? ' head-commit' : '') +
          (commit.successorInfo != null ? ' obsolete' : '')
        }
        data-testid={`commit-${commit.hash}`}>
        {commit.isHead || previewType === CommitPreview.GOTO_PREVIOUS_LOCATION ? (
          <HeadCommitInfo commit={commit} previewType={previewType} hasChildren={hasChildren} />
        ) : null}
        <div className="commit-rows">
          {isSelected ? (
            <div className="selected-commit-background" data-testid="selected-commit" />
          ) : null}
          <DraggableCommit
            className={
              'commit-details' + (previewType != null ? ` commit-preview-${previewType}` : '')
            }
            commit={commit}
            draggable={!isPublic && isDraggablePreview(previewType)}
            onClick={onClickToSelect}>
            <div className="commit-avatar" />
            {isPublic ? null : (
              <span className="commit-title">
                {commit.title}
                <span className="commit-date">
                  <RelativeDate date={commit.date} useShortVariant />
                </span>
              </span>
            )}
            <UnsavedEditedMessageIndicator commit={commit} />
            {commit.bookmarks.map(bookmark => (
              <VSCodeTag key={bookmark}>{bookmark}</VSCodeTag>
            ))}
            {commit.remoteBookmarks.map(remoteBookmarks => (
              <VSCodeTag key={remoteBookmarks}>{remoteBookmarks}</VSCodeTag>
            ))}
            {previewType === CommitPreview.REBASE_OPTIMISTIC_ROOT ? (
              <span className="commit-inline-operation-progress">
                <Icon icon="loading" /> <T>rebasing...</T>
              </span>
            ) : null}
            {previewType === CommitPreview.REBASE_ROOT ? (
              <>
                <VSCodeButton
                  appearance="secondary"
                  onClick={() => handlePreviewedOperation(/* cancel */ true)}>
                  <T>Cancel</T>
                </VSCodeButton>
                <VSCodeButton
                  appearance="primary"
                  onClick={() => handlePreviewedOperation(/* cancel */ false)}>
                  <T>Run Rebase</T>
                </VSCodeButton>
              </>
            ) : null}
            {actionsPrevented || commit.isHead ? null : (
              <span className="goto-button">
                <VSCodeButton
                  appearance="secondary"
                  onClick={event => {
                    runOperation(
                      new GotoOperation(
                        // If the commit has a remote bookmark, use that instead of the hash. This is easier to read in the command history
                        // and works better with optimistic state
                        commit.remoteBookmarks.length > 0 ? commit.remoteBookmarks[0] : commit.hash,
                      ),
                    );
                    event.stopPropagation(); // don't select commit
                  }}>
                  <T>Goto</T> <Icon icon="arrow-right" />
                </VSCodeButton>
              </span>
            )}
            {!isPublic && !actionsPrevented && commit.isHead ? <UncommitButton /> : null}
          </DraggableCommit>
          <DivIfChildren className="commit-second-row">
            {commit.diffId && !isPublic ? <DiffInfo diffId={commit.diffId} /> : null}
            {commit.successorInfo != null ? (
              <SuccessorInfoToDisplay successorInfo={commit.successorInfo} />
            ) : null}
          </DivIfChildren>
        </div>
      </div>
    );
  },
);

function DivIfChildren({
  children,
  ...props
}: React.DetailedHTMLProps<React.HTMLAttributes<HTMLDivElement>, HTMLDivElement>) {
  if (!children || (Array.isArray(children) && children.length === 0)) {
    return null;
  }
  return <div {...props}>{children}</div>;
}

function UnsavedEditedMessageIndicator({commit}: {commit: CommitInfo}) {
  const isEdted = useRecoilValue(hasUnsavedEditedCommitMessage(commit.hash));
  if (!isEdted) {
    return null;
  }
  return (
    <div className="unsaved-message-indicator" data-testid="unsaved-message-indicator">
      <Tooltip title={t('This commit has unsaved changes to its message')}>
        <Icon icon="circle-large-filled" />
      </Tooltip>
    </div>
  );
}

function HeadCommitInfo({
  commit,
  previewType,
  hasChildren,
}: {
  commit: CommitInfo;
  previewType?: CommitPreview;
  hasChildren: boolean;
}) {
  const uncommittedChanges = useRecoilValue(latestUncommittedChanges);

  // render head info indented when:
  //  - we have uncommitted changes, so we're showing files
  // and EITHER
  //    - we're on a public commit (you'll create a new "branch" by committing)
  //    - the commit we're rendering has children (we'll render the current child as new branch after committing)
  const indent = uncommittedChanges.length > 0 && (commit.phase === 'public' || hasChildren);

  return (
    <div className={`head-commit-info-container${indent ? ' head-commit-info-indented' : ''}`}>
      <YouAreHere previewType={previewType} />
      {
        commit.isHead ? (
          <div className="head-commit-info">
            <UncommittedChanges place="main" />
          </div>
        ) : null // don't show uncommitted changes twice while checking out
      }
      {indent ? <BranchIndicator /> : null}
    </div>
  );
}

export function YouAreHere({
  previewType,
  hideSpinner,
}: {
  previewType?: CommitPreview;
  hideSpinner?: boolean;
}) {
  const isFetching = useRecoilValue(isFetchingUncommittedChanges) && !hideSpinner;

  let text;
  let spinner = false;
  switch (previewType) {
    case CommitPreview.GOTO_DESTINATION:
      text = <T>You're moving here...</T>;
      spinner = true;
      break;
    case CommitPreview.GOTO_PREVIOUS_LOCATION:
      text = <T>You were here...</T>;
      spinner = true;
      break;
    default:
      text = <T>You are here</T>;
      break;
  }
  return (
    <div className="you-are-here-container">
      <span className="you-are-here">
        {spinner ? <Icon icon="loading" /> : null}
        {text}
      </span>
      {isFetching ? <Icon icon="loading" /> : null}
    </div>
  );
}

let commitBeingDragged: CommitInfo | undefined = undefined;

function preventDefault(e: Event) {
  e.preventDefault();
}
function handleDragEnd(event: Event) {
  event.preventDefault();

  commitBeingDragged = undefined;
  const draggedDOMNode = event.target;
  draggedDOMNode?.removeEventListener('dragend', handleDragEnd);
  document.removeEventListener('drop', preventDefault);
  document.removeEventListener('dragover', preventDefault);
}

function DraggableCommit({
  commit,
  children,
  className,
  draggable,
  onClick,
}: {
  commit: CommitInfo;
  children: React.ReactNode;
  className: string;
  draggable: boolean;
  onClick?: (e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>) => unknown;
}) {
  const handleDragEnter = useRecoilCallback(
    ({snapshot, set}) =>
      () => {
        const loadable = snapshot.getLoadable(latestCommitTreeMap);
        if (loadable.state !== 'hasValue') {
          return;
        }
        const treeMap = loadable.contents;

        if (commitBeingDragged != null && commit.hash !== commitBeingDragged.hash) {
          const draggedTree = treeMap.get(commitBeingDragged.hash);
          if (draggedTree) {
            if (
              // can't rebase a commit onto its descendants
              !isDescendant(commit.hash, draggedTree) &&
              // can't rebase a commit onto its parent... it's already there!
              !(commitBeingDragged.parents as Array<string>).includes(commit.hash)
            ) {
              // if the dest commit has a remote bookmark, use that instead of the hash.
              // this is easier to understand in the command history and works better with optimistic state
              const destination =
                commit.remoteBookmarks.length > 0 ? commit.remoteBookmarks[0] : commit.hash;
              set(
                operationBeingPreviewed,
                new RebaseOperation(commitBeingDragged.hash, destination),
              );
            }
          }
        }
      },
    [commit],
  );

  const handleDragStart = useRecoilCallback(
    ({snapshot}) =>
      (event: React.DragEvent<HTMLDivElement>) => {
        // can't rebase with uncommitted changes
        const loadable = snapshot.getLoadable(latestUncommittedChanges);
        const hasUncommittedChanges = loadable.state === 'hasValue' && loadable.contents.length > 0;

        if (hasUncommittedChanges) {
          event.preventDefault();
        }

        commitBeingDragged = commit;
        event.dataTransfer.dropEffect = 'none';

        const draggedDOMNode = event.target;
        // prevent animation of commit returning to drag start location on drop
        draggedDOMNode.addEventListener('dragend', handleDragEnd);
        document.addEventListener('drop', preventDefault);
        document.addEventListener('dragover', preventDefault);
      },
    [commit],
  );

  return (
    <div
      className={className}
      onDragStart={handleDragStart}
      onDragEnter={handleDragEnter}
      draggable={draggable}
      onClick={onClick}
      onKeyPress={event => {
        if (event.key === 'Enter') {
          onClick?.(event);
        }
      }}
      tabIndex={0}
      data-testid={'draggable-commit'}>
      {children}
    </div>
  );
}

export function SuccessorInfoToDisplay({successorInfo}: {successorInfo: SuccessorInfo}) {
  switch (successorInfo.type) {
    case 'land':
    case 'pushrebase':
      return <T>Landed as a newer commit</T>;
    case 'amend':
      return <T>Amended as a newer commit'</T>;
    case 'rebase':
      return <T>Rebased as a newer commit'</T>;
    case 'split':
      return <T>Split as a newer commit'</T>;
    case 'fold':
      return <T>Folded as a newer commit'</T>;
    case 'histedit':
      return <T>Histedited as a newer commit'</T>;
    default:
      return <T>Rewritten as a newer commit'</T>;
  }
}
