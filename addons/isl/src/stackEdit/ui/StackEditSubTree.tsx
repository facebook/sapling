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
import {useRef, useState} from 'react';
import {nullthrows} from 'shared/utils';
import {AnimatedReorderGroup} from '../../AnimatedReorderGroup';
import {CommitTitle as StandaloneCommitTitle} from '../../CommitTitle';
import {Row} from '../../ComponentUtils';
import {DragHandle} from '../../DragHandle';
import {DraggingOverlay} from '../../DraggingOverlay';
import {t, T} from '../../i18n';
import {SplitCommitIcon} from '../../icons/SplitCommitIcon';
import {reorderedRevs} from '../commitStackState';
import {ReorderState} from '../reorderState';
import {bumpStackEditMetric, useStackEditState, WDIR_NODE} from './stackEditState';

import './StackEditSubTree.css';

type ActivateSplitProps = {
  activateSplitTab?: () => void;
};

// <StackEditSubTree /> assumes stack is loaded.
export function StackEditSubTree(props: ActivateSplitProps): React.ReactElement {
  const stackEdit = useStackEditState();
  const [reorderState, setReorderState] = useState<ReorderState>(() => new ReorderState());

  const onDragRef = useRef<DragHandler | null>(null);
  const commitListDivRef = useRef<HTMLDivElement | null>(null);

  const commitStack = stackEdit.commitStack;
  const revs = reorderState.isDragging()
    ? reorderState.reorderRevs.slice(1).toArray().reverse()
    : commitStack.mutableRevs().reverse();

  // What will happen after drop.
  const draggingHintText: string | null =
    reorderState.draggingRevs.size > 1 ? t('Dependent commits are moved together') : null;

  const getDragHandler = (rev: CommitRev): DragHandler => {
    // Track `reorderState` updates in case the <DragHandle/>-captured `reorderState` gets outdated.
    // Note: this would be unnecessary if React provides `getState()` instead of `state`.
    let currentReorderState = reorderState;
    const setCurrentReorderState = (state: ReorderState) => {
      if (is(state, currentReorderState)) {
        return;
      }
      currentReorderState = state;
      setReorderState(state);
    };

    return (x, y, isDragging) => {
      // Visual update.
      onDragRef.current?.(x, y, isDragging);
      // State update.
      if (isDragging) {
        if (currentReorderState.isDragging()) {
          if (commitListDivRef.current) {
            const offset = calculateReorderOffset(
              commitListDivRef.current,
              y,
              currentReorderState.draggingRev,
            );
            const newReorderState = currentReorderState.withOffset(offset);
            setCurrentReorderState(newReorderState);
          }
        } else {
          setCurrentReorderState(ReorderState.init(commitStack, rev));
        }
      } else if (!isDragging && currentReorderState.isDragging()) {
        // Apply reorder.
        const order = currentReorderState.reorderRevs.toArray();
        const commitStack = stackEdit.commitStack;
        if (commitStack.canReorder(order) && !currentReorderState.isNoop()) {
          const newStackState = commitStack.reorder(order);
          stackEdit.push(newStackState, {
            name: 'move',
            offset: currentReorderState.offset,
            depCount: currentReorderState.draggingRevs.size - 1,
            commit: nullthrows(commitStack.stack.get(currentReorderState.draggingRev)),
          });
          bumpStackEditMetric('moveDnD');
        }
        // Reset reorder state.
        setCurrentReorderState(new ReorderState());
      }
    };
  };

  return (
    <>
      <div className="stack-edit-subtree" ref={commitListDivRef}>
        <AnimatedReorderGroup>
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
      {reorderState.isDragging() && (
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
      <DragHandle onDrag={onDrag}>
        <Icon icon="grabber" />
      </DragHandle>
      {buttons}
      {title}
      {rightSideButtons}
    </Row>
  );
}

/**
 * Calculate the reorder "offset" based on the y axis.
 *
 * This function assumes the stack rev 0 is used as the "public" (or "immutable")
 * commit that is not rendered. If that's no longer the case, adjust the
 * `invisibleRevCount` accordingly.
 *
 * This is done by counting how many `.commit`s are below the y axis.
 * If nothing is reordered, there should be `rev - invisibleRevCount` commits below.
 * The existing `rev`s on the `.commit`s are not considered, as they can be before
 * or after the reorder preview, which are noisy to consider.
 */
function calculateReorderOffset(
  container: HTMLDivElement,
  y: number,
  draggingRev: CommitRev,
  invisibleRevCount = 1,
): number {
  let belowCount = 0;
  const parentY: number = nullthrows(container).getBoundingClientRect().y;
  container.querySelectorAll('.commit').forEach(element => {
    const commitDiv = element as HTMLDivElement;
    // commitDiv.getBoundingClientRect() will consider the animation transform.
    // We don't want to be affected by animation, so we use 'container' here,
    // assuming 'container' is not animated. The 'container' can be in <ScrollY>,
    // and should have a 'relative' position.
    const commitY = parentY + commitDiv.offsetTop;
    if (commitY > y) {
      belowCount += 1;
    }
  });
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
