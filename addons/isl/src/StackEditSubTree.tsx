/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitStackState} from './stackEdit/commitStackState';
import type {Rev} from './stackEdit/fileStackState';
import type {UseStackEditState} from './stackEditState';

import {Tooltip} from './Tooltip';
import {t, T} from './i18n';
import {useStackEditState} from './stackEditState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {Icon} from 'shared/Icon';
import {unwrap} from 'shared/utils';

import './StackEditSubTree.css';

// <StackEditSubTree /> assumes stack is loaded.
export function StackEditSubTree(): React.ReactElement {
  const stackEdit = useStackEditState();

  const revs = stackEdit.commitStack.mutableRevs().reverse();

  return (
    <div className="stack-edit-subtree">
      {revs.map(rev => (
        <StackEditCommit key={rev} rev={rev} stackEdit={stackEdit} />
      ))}
    </div>
  );
}

export function StackEditCommit({
  rev,
  stackEdit,
}: {
  rev: Rev;
  stackEdit: UseStackEditState;
}): React.ReactElement {
  const state = stackEdit.commitStack;
  const canFold = state.canFoldDown(rev);
  const canDrop = state.canDrop(rev);
  const canMoveDown = state.canReorder(reorderedRevs(state, rev - 1));
  const canMoveUp = state.canReorder(reorderedRevs(state, rev));
  const commit = unwrap(state.stack.get(rev));
  const titleText = commit.text.split('\n', 1).at(0) ?? '';

  const handleMoveUp = () => stackEdit.push(state.reorder(reorderedRevs(state, rev)), t('Move up'));
  const handleMoveDown = () =>
    stackEdit.push(state.reorder(reorderedRevs(state, rev - 1)), t('Move down'));
  const handleFoldDown = () => stackEdit.push(state.foldDown(rev), t('Fold down'));
  const handleDrop = () => stackEdit.push(state.drop(rev), t('Drop'));

  const title =
    titleText === '' ? (
      <span className="commit-title untitled">
        <T>Untitled</T>
      </span>
    ) : (
      <span className="commit-title">{titleText}</span>
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

  return (
    <div className="commit">
      <div className="commit-rows">
        <div className="commit-avatar" />
        <div className="commit-details">
          {buttons}
          {title}
        </div>
      </div>
    </div>
  );
}

// Reorder rev and rev + 1.
function reorderedRevs(state: CommitStackState, rev: number): Rev[] {
  // Basically, `toSpliced`, but it's not avaialble everywhere.
  const order = state.revs();
  if (rev < 0 || rev >= order.length - 1) {
    // out of range - canReorder([]) will return false.
    return [];
  }
  const rev1 = order[rev];
  const rev2 = order[rev + 1];
  order.splice(rev, 2, rev2, rev1);
  return order;
}
