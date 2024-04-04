/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from './types';

import {latestSuccessorUnlessExplicitlyObsolete} from './SuccessionTracker';
import {Tooltip} from './Tooltip';
import {t} from './i18n';
import {readAtom, writeAtom} from './jotaiUtils';
import {RebaseOperation} from './operations/RebaseOperation';
import {operationBeingPreviewed} from './operationsState';
import {CommitPreview, uncommittedChangesWithPreviews} from './previews';
import {latestDag} from './serverAPIState';
import {succeedableRevset} from './types';
import {useState, useCallback, useEffect} from 'react';

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

let commitBeingDragged: CommitInfo | undefined = undefined;

// This is a global state outside React because commit DnD is a global
// concept: there won't be 2 DnD happening at once in the same window.
let lastDndId = 0;

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

export function DragToRebase({
  commit,
  previewType,
  children,
  className,
  onClick,
  onDoubleClick,
  onContextMenu,
}: {
  commit: CommitInfo;
  previewType: CommitPreview | undefined;
  children: React.ReactNode;
  className: string;
  onClick?: (e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>) => unknown;
  onDoubleClick?: (e: React.MouseEvent<HTMLDivElement>) => unknown;
  onContextMenu?: React.MouseEventHandler<HTMLDivElement>;
}) {
  const draggable = commit.phase !== 'public' && isDraggablePreview(previewType);
  const [dragDisabledMessage, setDragDisabledMessage] = useState<string | null>(null);
  const handleDragEnter = useCallback(() => {
    // Capture the environment.
    const currentBeingDragged = commitBeingDragged;
    const currentDndId = ++lastDndId;

    const handleDnd = () => {
      // Skip handling if there was a new "DragEnter" event that invalidates this one.
      if (lastDndId != currentDndId) {
        return;
      }
      const dag = readAtom(latestDag);

      if (currentBeingDragged != null && commit.hash !== currentBeingDragged.hash) {
        const beingDragged = currentBeingDragged;
        if (dag.has(beingDragged.hash)) {
          if (
            // can't rebase a commit onto its descendants
            !dag.isAncestor(beingDragged.hash, commit.hash) &&
            // can't rebase a commit onto its parent... it's already there!
            !(beingDragged.parents as Array<string>).includes(commit.hash)
          ) {
            // if the dest commit has a remote bookmark, use that instead of the hash.
            // this is easier to understand in the command history and works better with optimistic state
            const destination =
              commit.remoteBookmarks.length > 0
                ? succeedableRevset(commit.remoteBookmarks[0])
                : latestSuccessorUnlessExplicitlyObsolete(commit);
            writeAtom(operationBeingPreviewed, op => {
              const newRebase = new RebaseOperation(
                latestSuccessorUnlessExplicitlyObsolete(beingDragged),
                destination,
              );
              const isEqual = newRebase.equals(op);
              return isEqual ? op : newRebase;
            });
          }
        }
      }
    };

    // This allows us to recieve a list of "queued" DragEnter events
    // before actually handling them. This way we can skip "invalidated"
    // events and only handle the last (valid) one.
    window.setTimeout(() => {
      handleDnd();
    }, 1);
  }, [commit]);

  const handleDragStart = useCallback(
    (event: React.DragEvent<HTMLDivElement>) => {
      // can't rebase with uncommitted changes
      if (hasUncommittedChanges()) {
        setDragDisabledMessage(t('Cannot drag to rebase with uncommitted changes.'));
        event.preventDefault();
      }
      if (commit.successorInfo != null) {
        setDragDisabledMessage(t('Cannot rebase obsoleted commits.'));
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

  useEffect(() => {
    if (dragDisabledMessage) {
      const timeout = setTimeout(() => setDragDisabledMessage(null), 1500);
      return () => clearTimeout(timeout);
    }
  }, [dragDisabledMessage]);

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
      data-testid={'draggable-commit'}>
      <div className="commit-wide-drag-target" onDragEnter={handleDragEnter} />
      {dragDisabledMessage != null ? (
        <Tooltip trigger="manual" shouldShow title={dragDisabledMessage}>
          {children}
        </Tooltip>
      ) : (
        children
      )}
    </div>
  );
}

function hasUncommittedChanges(): boolean {
  const changes = readAtom(uncommittedChangesWithPreviews);
  return (
    changes.filter(
      commit => commit.status !== '?', // untracked files are ok
    ).length > 0
  );
}
