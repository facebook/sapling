/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, SuccessorInfo} from './types';

import {BranchIndicator} from './BranchIndicator';
import {hasUnsavedEditedCommitMessage} from './CommitInfoView/CommitInfoState';
import {currentComparisonMode} from './ComparisonView/atoms';
import {highlightedCommits} from './HighlightedCommits';
import {InlineBadge} from './InlineBadge';
import {Tooltip} from './Tooltip';
import {UncommitButton} from './UncommitButton';
import {UncommittedChanges} from './UncommittedChanges';
import {DiffInfo} from './codeReview/DiffBadge';
import {islDrawerState} from './drawerState';
import {isDescendant} from './getCommitTree';
import {t, T} from './i18n';
import {GotoOperation} from './operations/GotoOperation';
import {HideOperation} from './operations/HideOperation';
import {RebaseOperation} from './operations/RebaseOperation';
import platform from './platform';
import {CommitPreview} from './previews';
import {RelativeDate} from './relativeDate';
import {isNarrowCommitTree} from './responsive';
import {useCommitSelection} from './selection';
import {
  isFetchingUncommittedChanges,
  latestCommitTreeMap,
  latestUncommittedChanges,
  operationBeingPreviewed,
  useRunOperation,
  useRunPreviewedOperation,
} from './serverAPIState';
import {short} from './utils';
import {VSCodeButton, VSCodeTag} from '@vscode/webview-ui-toolkit/react';
import React, {memo} from 'react';
import {useRecoilCallback, useRecoilValue, useSetRecoilState} from 'recoil';
import {ComparisonType} from 'shared/Comparison';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {notEmpty} from 'shared/utils';

function isDraggablePreview(previewType?: CommitPreview): boolean {
  switch (previewType) {
    // dragging preview descendants would be confusing (it would reset part of your drag),
    // you probably meant to drag the root.
    case CommitPreview.REBASE_DESCENDANT:
    // old commits are already being dragged
    case CommitPreview.REBASE_OLD:
    case CommitPreview.HIDDEN_ROOT:
    case CommitPreview.HIDDEN_DESCENDANT:
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
    case CommitPreview.HIDDEN_ROOT:
    case CommitPreview.HIDDEN_DESCENDANT:
    case CommitPreview.NON_ACTIONABLE_COMMIT:
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
    const setDrawerState = useSetRecoilState(islDrawerState);
    const isPublic = commit.phase === 'public';

    const handlePreviewedOperation = useRunPreviewedOperation();
    const runOperation = useRunOperation();
    const isHighlighted = useRecoilValue(highlightedCommits).has(commit.hash);

    const {isSelected, onClickToSelect} = useCommitSelection(commit.hash);
    const actionsPrevented = previewPreventsActions(previewType);

    const isNarrow = useRecoilValue(isNarrowCommitTree);

    const isNonActionable = previewType === CommitPreview.NON_ACTIONABLE_COMMIT;

    function onDoubleClickToShowDrawer(e: React.MouseEvent<HTMLDivElement>) {
      // Select the commit if it was deselected.
      if (!isSelected) {
        onClickToSelect(e);
      }
      // Show the drawer.
      setDrawerState(state => ({
        ...state,
        right: {
          ...state.right,
          collapsed: false,
        },
      }));
    }
    const setOperationBeingPreviewed = useSetRecoilState(operationBeingPreviewed);

    const viewChangesCallback = useRecoilCallback(({set}) => () => {
      set(currentComparisonMode, {
        comparison: {type: ComparisonType.Committed, hash: commit.hash},
        visible: true,
      });
    });

    const contextMenu = useContextMenu(() => {
      const items = [
        {
          label: <T replace={{$hash: short(commit?.hash)}}>Copy Commit Hash "$hash"</T>,
          onClick: () => platform.clipboardCopy(commit.hash),
        },
      ];
      if (!isPublic) {
        items.push({
          label: <T>View Changes in Commit</T>,
          onClick: viewChangesCallback,
        });
      }
      if (!isPublic && !actionsPrevented) {
        items.push({
          label: <T>Hide Commit and Descendents</T>,
          onClick: () => setOperationBeingPreviewed(new HideOperation(commit.hash)),
        });
      }
      return items;
    });

    const commitActions = [];

    if (previewType === CommitPreview.REBASE_ROOT) {
      commitActions.push(
        <React.Fragment key="rebase">
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
        </React.Fragment>,
      );
    } else if (previewType === CommitPreview.HIDDEN_ROOT) {
      commitActions.push(
        <React.Fragment key="hide">
          <VSCodeButton
            appearance="secondary"
            onClick={() => handlePreviewedOperation(/* cancel */ true)}>
            <T>Cancel</T>
          </VSCodeButton>
          <VSCodeButton
            appearance="primary"
            onClick={() => handlePreviewedOperation(/* cancel */ false)}>
            <T>Hide</T>
          </VSCodeButton>
        </React.Fragment>,
      );
    }
    if (!actionsPrevented && !commit.isHead) {
      commitActions.push(
        <span className="goto-button" key="goto-button">
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
        </span>,
      );
    }
    if (!isPublic && !actionsPrevented && commit.isHead) {
      commitActions.push(<UncommitButton key="uncommit" />);
    }

    return (
      <div
        className={
          'commit' +
          (commit.isHead ? ' head-commit' : '') +
          (commit.successorInfo != null ? ' obsolete' : '') +
          (isHighlighted ? ' highlighted' : '') +
          (isPublic || hasChildren ? '' : ' topmost')
        }
        data-testid={`commit-${commit.hash}`}>
        {!isNonActionable &&
        (commit.isHead || previewType === CommitPreview.GOTO_PREVIOUS_LOCATION) ? (
          <HeadCommitInfo commit={commit} previewType={previewType} hasChildren={hasChildren} />
        ) : null}
        <div
          className={
            'commit-rows' + (isNarrow ? ' narrow' : '') + (isSelected ? ' selected-commit' : '')
          }>
          {isSelected ? (
            <div className="selected-commit-background" data-testid="selected-commit" />
          ) : null}
          <DraggableCommit
            className={
              'commit-details' + (previewType != null ? ` commit-preview-${previewType}` : '')
            }
            commit={commit}
            draggable={!isPublic && isDraggablePreview(previewType)}
            onClick={onClickToSelect}
            onContextMenu={contextMenu}
            onDoubleClick={onDoubleClickToShowDrawer}>
            <div className="commit-avatar" />
            {isPublic ? null : (
              <span className="commit-title">
                <span>{commit.title}</span>
                <CommitDate date={commit.date} />
              </span>
            )}
            <UnsavedEditedMessageIndicator commit={commit} />
            {commit.bookmarks.map(bookmark => (
              <VSCodeTag key={bookmark}>{bookmark}</VSCodeTag>
            ))}
            {commit.remoteBookmarks.map(remoteBookmarks => (
              <VSCodeTag key={remoteBookmarks}>{remoteBookmarks}</VSCodeTag>
            ))}
            {commit?.stableCommitMetadata != null ? (
              <Tooltip title={commit.stableCommitMetadata}>
                <div className="stable-commit-metadata">
                  <VSCodeTag key={commit.stableCommitMetadata}>
                    {commit.stableCommitMetadata}
                  </VSCodeTag>
                </div>
              </Tooltip>
            ) : null}
            {isPublic ? <CommitDate date={commit.date} /> : null}
            {previewType === CommitPreview.REBASE_OPTIMISTIC_ROOT ? (
              <span className="commit-inline-operation-progress">
                <Icon icon="loading" /> <T>rebasing...</T>
              </span>
            ) : null}
            {isNarrow ? commitActions : null}
          </DraggableCommit>
          <DivIfChildren className="commit-second-row">
            {commit.diffId && !isPublic ? <DiffInfo diffId={commit.diffId} /> : null}
            {commit.successorInfo != null ? (
              <SuccessorInfoToDisplay successorInfo={commit.successorInfo} />
            ) : null}
          </DivIfChildren>
          {!isNarrow ? commitActions : null}
        </div>
      </div>
    );
  },
);

function CommitDate({date}: {date: Date}) {
  return (
    <span className="commit-date">
      <RelativeDate date={date} useShortVariant />
    </span>
  );
}

function DivIfChildren({
  children,
  ...props
}: React.DetailedHTMLProps<React.HTMLAttributes<HTMLDivElement>, HTMLDivElement>) {
  if (!children || (Array.isArray(children) && children.filter(notEmpty).length === 0)) {
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
      break;
    default:
      text = <T>You are here</T>;
      break;
  }
  return (
    <div className="you-are-here-container">
      <InlineBadge kind="primary">
        {spinner ? <Icon icon="loading" /> : null}
        {text}
      </InlineBadge>
      {isFetching &&
      // don't show fetch spinner on previous location
      previewType !== CommitPreview.GOTO_PREVIOUS_LOCATION ? (
        <Icon icon="loading" />
      ) : null}
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
  onDoubleClick,
  onContextMenu,
}: {
  commit: CommitInfo;
  children: React.ReactNode;
  className: string;
  draggable: boolean;
  onClick?: (e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>) => unknown;
  onDoubleClick?: (e: React.MouseEvent<HTMLDivElement>) => unknown;
  onContextMenu?: React.MouseEventHandler<HTMLDivElement>;
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
      onDoubleClick={onDoubleClick}
      onKeyPress={event => {
        if (event.key === 'Enter') {
          onClick?.(event);
        }
      }}
      onContextMenu={onContextMenu}
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
      return <T>Amended as a newer commit</T>;
    case 'rebase':
      return <T>Rebased as a newer commit</T>;
    case 'split':
      return <T>Split as a newer commit</T>;
    case 'fold':
      return <T>Folded as a newer commit</T>;
    case 'histedit':
      return <T>Histedited as a newer commit</T>;
    default:
      return <T>Rewritten as a newer commi'</T>;
  }
}
