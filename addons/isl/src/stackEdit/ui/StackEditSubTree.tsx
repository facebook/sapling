/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DragHandler} from '../../DragHandle';
import type {CommitState} from '../commitStackState';
import type {Rev} from '../fileStackState';
import type {StackEditOpDescription, UseStackEditState} from './stackEditState';

import {AnimatedReorderGroup} from '../../AnimatedReorderGroup';
import {CommitTitle as StandaloneCommitTitle} from '../../CommitTitle';
import {FlexRow} from '../../ComponentUtils';
import {DragHandle} from '../../DragHandle';
import {Tooltip} from '../../Tooltip';
import {t, T} from '../../i18n';
import {SplitCommitIcon} from '../../icons/SplitCommitIcon';
import {reorderedRevs} from '../commitStackState';
import {ReorderState} from '../reorderState';
import {bumpStackEditMetric, useStackEditState} from './stackEditState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {is} from 'immutable';
import {useRef, useState} from 'react';
import {Icon} from 'shared/Icon';
import {unwrap} from 'shared/utils';

import './StackEditSubTree.css';

type ActivateSplitProps = {
  activateSplitTab?: () => void;
};

// <StackEditSubTree /> assumes stack is loaded.
export function StackEditSubTree(props: ActivateSplitProps): React.ReactElement {
  const stackEdit = useStackEditState();
  const [reorderState, setReorderState] = useState<ReorderState>(() => new ReorderState());

  const draggingDivRef = useRef<HTMLDivElement | null>(null);
  const commitListDivRef = useRef<HTMLDivElement | null>(null);

  const commitStack = stackEdit.commitStack;
  const revs = reorderState.isDragging()
    ? reorderState.reorderRevs.slice(1).toArray().reverse()
    : commitStack.mutableRevs().reverse();

  // What will happen after drop.
  const draggingHintText: string | null =
    reorderState.draggingRevs.size > 1 ? t('Dependent commits are moved together') : null;

  const getDragHandler = (rev: Rev): DragHandler => {
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
      const draggingDiv = draggingDivRef.current;
      if (draggingDiv != null) {
        if (isDragging) {
          Object.assign(draggingDiv.style, {
            transform: `translate(${x}px, calc(-50% + ${y}px))`,
            opacity: '1',
          });
        } else {
          draggingDiv.style.opacity = '0';
        }
      }
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
            commit: unwrap(commitStack.stack.get(currentReorderState.draggingRev)),
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
        <div className="stack-edit-dragging" ref={draggingDivRef}>
          <div className="stack-edit-dragging-commit-list">
            {reorderState.draggingRevs
              .toArray()
              .reverse()
              .map(rev => (
                <StackEditCommit key={rev} rev={rev} stackEdit={stackEdit} />
              ))}
          </div>
          {draggingHintText && (
            <div className="stack-edit-dragging-hint-container">
              <span className="stack-edit-dragging-hint-text tooltip">{draggingHintText}</span>
            </div>
          )}
        </div>
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
  rev: Rev;
  stackEdit: UseStackEditState;
  onDrag?: DragHandler;
  isReorderPreview?: boolean;
} & ActivateSplitProps): React.ReactElement {
  const state = stackEdit.commitStack;
  const canFold = state.canFoldDown(rev);
  const canDrop = state.canDrop(rev);
  const canMoveDown = state.canMoveDown(rev);
  const canMoveUp = state.canMoveUp(rev);
  const commit = unwrap(state.stack.get(rev));
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
        <VSCodeButton disabled={!canMoveUp} onClick={handleMoveUp} appearance="icon">
          <Icon icon="chevron-up" />
        </VSCodeButton>
      </Tooltip>
      <Tooltip
        title={
          canMoveDown
            ? t('Move commit down in the stack')
            : t(
                'Cannot move up if this commit is at the bottom, or if this commit depends on its parent',
              )
        }>
        <VSCodeButton disabled={!canMoveDown} onClick={handleMoveDown} appearance="icon">
          <Icon icon="chevron-down" />
        </VSCodeButton>
      </Tooltip>
      <Tooltip
        title={
          canFold
            ? t('Fold the commit with its parent')
            : t('Can not fold with parent if this commit is at the bottom')
        }>
        <VSCodeButton disabled={!canFold} onClick={handleFoldDown} appearance="icon">
          <Icon icon="fold-down" />
        </VSCodeButton>
      </Tooltip>
      <Tooltip
        title={
          canDrop
            ? t('Drop the commit in the stack')
            : t('Cannot drop this commit because it has dependencies')
        }>
        <VSCodeButton disabled={!canDrop} onClick={handleDrop} appearance="icon">
          <Icon icon="close" />
        </VSCodeButton>
      </Tooltip>
    </div>
  );

  const rightSideButtons = (
    <div className="stack-edit-right-side-buttons">
      <Tooltip title={t('Start interactive split for this commit')}>
        <VSCodeButton onClick={handleSplit} appearance="icon">
          <SplitCommitIcon slot="start" />
          <T>Split</T>
        </VSCodeButton>
      </Tooltip>
    </div>
  );

  return (
    <FlexRow
      data-reorder-id={onDrag ? commit.key : ''}
      data-rev={rev}
      className={`commit${isReorderPreview ? ' commit-reorder-preview' : ''}`}>
      <DragHandle onDrag={onDrag}>
        <Icon icon="grabber" />
      </DragHandle>
      {buttons}
      {title}
      {rightSideButtons}
    </FlexRow>
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
  draggingRev: Rev,
  invisibleRevCount = 1,
): number {
  let belowCount = 0;
  const parentY: number = unwrap(container).getBoundingClientRect().y;
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
  }
  return <T>unknown</T>;
}

/** Used in undo tooltip. Styled. */
function CommitTitle({commit}: {commit: CommitState}): React.ReactElement {
  return <span className="commit-title">{commit.text.split('\n', 1).at(0)}</span>;
}
