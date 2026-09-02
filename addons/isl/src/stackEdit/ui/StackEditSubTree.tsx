/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DragHandler} from '../../DragHandle';
import type {CommitRev, CommitState} from '../commitStackState';
import type {StackEditOpDescription, UseStackEditState} from './stackEditState';

import {is} from 'immutable';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {useLayoutEffect, useRef, useState} from 'react';
import {nullthrows} from 'shared/utils';
import {AnimatedReorderGroup} from '../../AnimatedReorderGroup';
import {CommitTitle as StandaloneCommitTitle} from '../../CommitTitle';
import {Row} from '../../ComponentUtils';
import {DragHandle} from '../../DragHandle';
import {DraggingOverlay} from '../../DraggingOverlay';
import {codeReviewProvider, diffSummary} from '../../codeReview/CodeReviewInfo';
import {DiffBadge} from '../../codeReview/DiffBadge';
import {t, T} from '../../i18n';
import {SplitCommitIcon} from '../../icons/SplitCommitIcon';
import {commitByHash} from '../../serverAPIState';
import {reorderedRevs} from '../commitStackState';
import {ReorderState} from '../reorderState';
import {bumpStackEditMetric, useStackEditState, WDIR_NODE} from './stackEditState';

import './StackEditSubTree.css';

type ActivateSplitProps = {
  activateSplitTab?: () => void;
};

class StackReorderDragController {
  private reorderState = new ReorderState();
  private reorderTops: ReadonlyArray<number> = [];
  private stackEdit: UseStackEditState | null = null;

  constructor(private publishState: (state: ReorderState) => void) {}

  setStackEdit(stackEdit: UseStackEditState) {
    this.stackEdit = stackEdit;
  }

  setGeometry(tops: ReadonlyArray<number>) {
    this.reorderTops = tops;
  }

  handleDrag(rev: CommitRev, y: number, isDragging: boolean, container: HTMLElement | null) {
    const stackEdit = this.stackEdit;
    if (stackEdit == null) {
      return;
    }
    if (!isDragging) {
      this.finish(stackEdit);
      return;
    }
    if (!this.reorderState.isDragging()) {
      this.setReorderState(ReorderState.init(stackEdit.commitStack, rev));
      return;
    }
    if (container == null || this.reorderTops.length === 0) {
      return;
    }
    const relativeY = y - container.getBoundingClientRect().top;
    const offset = calculateReorderOffset(
      this.reorderTops,
      relativeY,
      this.reorderState.draggingRev,
    );
    if (offset !== this.reorderState.offset) {
      this.setReorderState(this.reorderState.withOffset(offset));
    }
  }

  private finish(stackEdit: UseStackEditState) {
    if (!this.reorderState.isDragging()) {
      return;
    }
    const order = this.reorderState.reorderRevs.toArray();
    const commitStack = stackEdit.commitStack;
    if (commitStack.canReorder(order) && !this.reorderState.isNoop()) {
      const newStackState = commitStack.reorder(order);
      stackEdit.push(newStackState, {
        name: 'move',
        offset: this.reorderState.offset,
        depCount: this.reorderState.draggingRevs.size - 1,
        commit: nullthrows(commitStack.stack.get(this.reorderState.draggingRev)),
      });
      bumpStackEditMetric('moveDnD');
    }
    this.reorderTops = [];
    this.setReorderState(new ReorderState());
  }

  private setReorderState(state: ReorderState) {
    if (is(state, this.reorderState)) {
      return;
    }
    this.reorderState = state;
    this.publishState(state);
  }
}

// <StackEditSubTree /> assumes stack is loaded.
export function StackEditSubTree(props: ActivateSplitProps): React.ReactElement {
  const stackEdit = useStackEditState();
  const [reorderState, setReorderState] = useState<ReorderState>(() => new ReorderState());

  const onDragRef = useRef<DragHandler | null>(null);
  const commitListDivRef = useRef<HTMLDivElement | null>(null);
  const dragControllerRef = useRef<StackReorderDragController | null>(null);
  if (dragControllerRef.current == null) {
    dragControllerRef.current = new StackReorderDragController(setReorderState);
  }
  const dragController = dragControllerRef.current;

  const commitStack = stackEdit.commitStack;
  const isReordering = reorderState.isDragging();
  const revs = isReordering
    ? reorderState.reorderRevs.slice(1).toArray().reverse()
    : commitStack.mutableRevs().reverse();

  // What will happen after drop.
  const draggingHintText: string | null =
    reorderState.draggingRevs.size > 1 ? t('Dependent commits are moved together') : null;

  useLayoutEffect(() => {
    dragController.setStackEdit(stackEdit);
  });

  useLayoutEffect(() => {
    if (!isReordering) {
      dragController.setGeometry([]);
      return;
    }
    const container = commitListDivRef.current;
    if (container != null) {
      dragController.setGeometry(snapshotCommitTops(container));
    }
  }, [commitStack, dragController, isReordering, reorderState.offset]);

  const getDragHandler = (rev: CommitRev): DragHandler => {
    return (x, y, isDragging) => {
      onDragRef.current?.(x, y, isDragging);
      dragController.handleDrag(rev, y, isDragging, commitListDivRef.current);
    };
  };

  return (
    <>
      <div className="stack-edit-subtree" ref={commitListDivRef}>
        <AnimatedReorderGroup disableAnimation={isReordering}>
          {revs.map(rev => {
            return (
              <StackEditCommit
                key={rev}
                rev={rev}
                stackEdit={stackEdit}
                isReorderPreview={reorderState.draggingRevs.includes(rev)}
                onDrag={getDragHandler(rev)}
                activateSplitTab={props.activateSplitTab}
              />
            );
          })}
        </AnimatedReorderGroup>
      </div>
      {isReordering && (
        <DraggingOverlay onDragRef={onDragRef} hint={draggingHintText}>
          {reorderState.draggingRevs
            .toArray()
            .reverse()
            .map(rev => (
              <StackEditCommit key={rev} rev={rev} stackEdit={stackEdit} />
            ))}
        </DraggingOverlay>
      )}
    </>
  );
}

export function StackEditCommit({
  rev,
  stackEdit,
  onDrag,
  isReorderPreview,
  activateSplitTab,
}: {
  rev: CommitRev;
  stackEdit: UseStackEditState;
  onDrag?: DragHandler;
  isReorderPreview?: boolean;
} & ActivateSplitProps): React.ReactElement {
  const state = stackEdit.commitStack;
  const canFold = state.canFoldDown(rev);
  const canDrop = state.canDrop(rev);
  const canMoveDown = state.canMoveDown(rev);
  const canMoveUp = state.canMoveUp(rev);
  const commit = nullthrows(state.stack.get(rev));
  const titleText = commit.text.split('\n', 1).at(0) ?? '';

  const handleMoveUp = () => {
    stackEdit.push(state.reorder(reorderedRevs(state, rev)), {name: 'move', offset: 1, commit});
    bumpStackEditMetric('moveUpDown');
  };
  const handleMoveDown = () => {
    stackEdit.push(state.reorder(reorderedRevs(state, rev - 1)), {
      name: 'move',
      offset: -1,
      commit,
    });
    bumpStackEditMetric('moveUpDown');
  };
  const handleFoldDown = () => {
    stackEdit.push(state.foldDown(rev), {name: 'fold', commit});
    bumpStackEditMetric('fold');
  };
  const handleDrop = () => {
    stackEdit.push(state.drop(rev), {name: 'drop', commit});
    bumpStackEditMetric('drop');
  };
  const handleSplit = () => {
    stackEdit.setSplitRange(commit.key);
    // Focus the split panel.
    activateSplitTab?.();
  };

  const title =
    titleText === '' ? (
      <span className="commit-title untitled">
        <T>Untitled</T>
      </span>
    ) : (
      <StandaloneCommitTitle commitMessage={commit.text} />
    );
  const buttons = (
    <div className="stack-edit-button-group">
      <Tooltip
        title={
          canMoveUp
            ? t('Move commit up in the stack')
            : t(
                'Cannot move up if this commit is at the top, or if the next commit depends on this commit',
              )
        }>
        <Button disabled={!canMoveUp} onClick={handleMoveUp} icon>
          <Icon icon="chevron-up" />
        </Button>
      </Tooltip>
      <Tooltip
        title={
          canMoveDown
            ? t('Move commit down in the stack')
            : t(
                'Cannot move up if this commit is at the bottom, or if this commit depends on its parent',
              )
        }>
        <Button disabled={!canMoveDown} onClick={handleMoveDown} icon>
          <Icon icon="chevron-down" />
        </Button>
      </Tooltip>
      <Tooltip
        title={
          canFold
            ? t('Fold the commit with its parent')
            : t('Can not fold with parent if this commit is at the bottom')
        }>
        <Button disabled={!canFold} onClick={handleFoldDown} icon>
          <Icon icon="fold-down" />
        </Button>
      </Tooltip>
      <Tooltip
        title={
          canDrop
            ? t('Drop the commit in the stack')
            : t('Cannot drop this commit because it has dependencies')
        }>
        <Button disabled={!canDrop} onClick={handleDrop} icon>
          <Icon icon="close" />
        </Button>
      </Tooltip>
    </div>
  );

  const rightSideButtons = (
    <div className="stack-edit-right-side-buttons">
      <Tooltip title={t('Start interactive split for this commit')}>
        <Button onClick={handleSplit} icon>
          <SplitCommitIcon slot="start" />
          <T>Split</T>
        </Button>
      </Tooltip>
    </div>
  );

  return (
    <Row
      data-reorder-id={onDrag ? commit.key : ''}
      data-rev={rev}
      className={`commit${isReorderPreview ? ' commit-reorder-preview' : ''}`}>
      <div className="stack-edit-controls">
        <DragHandle onDrag={onDrag}>
          <Icon icon="grabber" />
        </DragHandle>
        {buttons}
        {rightSideButtons}
      </div>
      <div className="stack-edit-content">
        {title}
        <StackEditDiffBadge commit={commit} />
      </div>
    </Row>
  );
}

/** Show the diff review status badge (e.g. "Accepted", "Needs Review") for a commit in the stack edit. */
function StackEditDiffBadge({commit}: {commit: CommitState}): React.ReactElement | null {
  const provider = useAtomValue(codeReviewProvider);
  const hash = commit.originalNodes.first();
  const commitInfo = useAtomValue(commitByHash(hash ?? ''));
  const diffId = commitInfo?.diffId;
  const diffResult = useAtomValue(diffSummary(diffId));

  if (provider == null || hash == null || diffId == null || diffResult?.value == null) {
    return null;
  }

  return <DiffBadge provider={provider} diff={diffResult.value} url={diffResult.value.url} />;
}

/**
 * Calculate the reorder "offset" based on the y axis.
 *
 * This function assumes the stack rev 0 is used as the "public" (or "immutable")
 * commit that is not rendered. If that's no longer the case, adjust the
 * `invisibleRevCount` accordingly.
 *
 * This is done by counting how many cached `.commit` tops are below the y axis.
 * If nothing is reordered, there should be `rev - invisibleRevCount` commits below.
 * The existing `rev`s on the `.commit`s are not considered, as they can be before
 * or after the reorder preview, which are noisy to consider.
 */
function snapshotCommitTops(container: HTMLDivElement): ReadonlyArray<number> {
  const containerTop = container.getBoundingClientRect().top;
  return [...container.querySelectorAll<HTMLElement>('.commit')]
    .map(element => element.getBoundingClientRect().top - containerTop)
    .sort((a, b) => a - b);
}

function calculateReorderOffset(
  tops: ReadonlyArray<number>,
  y: number,
  draggingRev: CommitRev,
  invisibleRevCount = 1,
): number {
  let low = 0;
  let high = tops.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (tops[middle] <= y) {
      low = middle + 1;
    } else {
      high = middle;
    }
  }
  const belowCount = tops.length - low;
  const offset = invisibleRevCount + belowCount - draggingRev;
  return offset;
}

/** Used in undo tooltip. */
export function UndoDescription({op}: {op?: StackEditOpDescription}): React.ReactElement | null {
  if (op == null) {
    return <T>null</T>;
  }
  if (op.name === 'move') {
    const {offset, commit} = op;
    const depCount = op.depCount ?? 0;
    const replace = {
      $commit: <CommitTitle commit={commit} />,
      $depCount: depCount,
      $offset: Math.abs(offset).toString(),
    };
    if (offset === 1) {
      return <T replace={replace}>moving up $commit</T>;
    } else if (offset === -1) {
      return <T replace={replace}>moving down $commit</T>;
    } else if (offset > 0) {
      if (depCount > 0) {
        return <T replace={replace}>moving up $commit and $depCount more</T>;
      } else {
        return <T replace={replace}>moving up $commit by $offset commits</T>;
      }
    } else {
      if (depCount > 0) {
        return <T replace={replace}>moving down $commit and $depCount more</T>;
      } else {
        return <T replace={replace}>moving down $commit by $offset commits</T>;
      }
    }
  } else if (op.name === 'swap') {
    return <T>swap the order of two commits</T>;
  } else if (op.name === 'fold') {
    const replace = {$commit: <CommitTitle commit={op.commit} />};
    return <T replace={replace}>folding down $commit</T>;
  } else if (op.name === 'insertBlankCommit') {
    return <T>inserting a new blank commit</T>;
  } else if (op.name === 'removeEmptyCommit') {
    return <T>removing an empty commit</T>;
  } else if (op.name === 'drop') {
    const replace = {$commit: <CommitTitle commit={op.commit} />};
    return <T replace={replace}>dropping $commit</T>;
  } else if (op.name === 'metaedit') {
    const replace = {$commit: <CommitTitle commit={op.commit} />};
    return <T replace={replace}>editing message of $commit</T>;
  } else if (op.name === 'import') {
    return <T>import</T>;
  } else if (op.name === 'fileStack') {
    return <T replace={{$file: op.fileDesc}}>editing file stack: $file</T>;
  } else if (op.name === 'split') {
    return <T replace={{$file: op.path}}>editing $file via interactive split</T>;
  } else if (op.name === 'splitWithAI') {
    return <T>split with AI</T>;
  } else if (op.name === 'absorbMove') {
    const replace = {$commit: <CommitTitle commit={op.commit} />};
    return <T replace={replace}>moving a diff chunk to $commit</T>;
  }
  return <T>unknown</T>;
}

/** Used in undo tooltip. Styled. */
function CommitTitle({commit}: {commit: CommitState}): React.ReactElement {
  if (commit.originalNodes.contains(WDIR_NODE)) {
    return <T>the working copy</T>;
  }
  return <span className="commit-title">{commit.text.split('\n', 1).at(0)}</span>;
}
