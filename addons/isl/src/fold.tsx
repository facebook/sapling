/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, Hash} from './types';

import {editedCommitMessages} from './CommitInfoView/CommitInfoState';
import {
  applyEditedFields,
  commitMessageFieldsSchema,
  commitMessageFieldsToString,
  mergeManyCommitMessageFields,
  parseCommitMessageFields,
} from './CommitInfoView/CommitMessageFields';
import {Tooltip} from './Tooltip';
import {T, t} from './i18n';
import {readAtom, writeAtom} from './jotaiUtils';
import {
  FOLD_COMMIT_PREVIEW_HASH_PREFIX,
  FoldOperation,
  getFoldRangeCommitHash,
} from './operations/FoldOperation';
import {type Dag, dagWithPreviewsJotai} from './previews';
import {selectedCommits} from './selection';
import {operationBeingPreviewedJotai, useRunPreviewedOperation} from './serverAPIState';
import {firstOfIterable} from './utils';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {atom, useAtomValue} from 'jotai';
import {useCallback} from 'react';
import {Icon} from 'shared/Icon';

/**
 * If the selected commits are linear, contiguous, and non-branching, they may be folded together.
 * This selector gives the range of commits that can be folded, if any,
 * so a button may be shown to do the fold.
 */
export const foldableSelection = atom(get => {
  const selection = get(selectedCommits);
  if (selection.size < 2) {
    return undefined;
  }
  const dag = get(dagWithPreviewsJotai);
  const foldable = getFoldableRange(selection, dag);
  return foldable;
});

/**
 * Given a set of selected commits, order them into an array from bottom to top.
 * If commits are not contiguous, returns undefined.
 * This selection must be linear and contiguous: no branches out are allowed.
 * This constitutes a set of commits that may be "folded"/combined into a single commit via `sl fold`.
 */
export function getFoldableRange(selection: Set<Hash>, dag: Dag): Array<CommitInfo> | undefined {
  const set = dag.present(selection);
  if (set.size <= 1) {
    return undefined;
  }
  const head = dag.heads(set);
  if (
    head.size !== 1 ||
    dag.roots(set).size !== 1 ||
    dag.merge(set).size > 0 ||
    dag.public_(set).size > 0 ||
    // only head can have other children
    dag.children(set.subtract(head)).subtract(set).size > 0
  ) {
    return undefined;
  }
  return dag.getBatch(dag.sortAsc(selection, {gap: false}));
}

export function FoldButton({commit}: {commit?: CommitInfo}) {
  const foldable = useAtomValue(foldableSelection);
  const onClick = useCallback(() => {
    if (foldable == null) {
      return;
    }
    const schema = readAtom(commitMessageFieldsSchema);
    if (schema == null) {
      return;
    }
    const messageFields = mergeManyCommitMessageFields(
      schema,
      foldable.map(commit => parseCommitMessageFields(schema, commit.title, commit.description)),
    );
    const message = commitMessageFieldsToString(schema, messageFields);
    writeAtom(operationBeingPreviewedJotai, new FoldOperation(foldable, message));
    writeAtom(selectedCommits, new Set([getFoldRangeCommitHash(foldable, /* isPreview */ true)]));
  }, [foldable]);
  if (foldable == null || (commit != null && foldable?.[0]?.hash !== commit.hash)) {
    return null;
  }
  return (
    <Tooltip title={t('Combine selected commits into one commit')}>
      <VSCodeButton appearance="secondary" onClick={onClick}>
        <Icon icon="fold" slot="start" />
        <T replace={{$count: foldable.length}}>Combine $count commits</T>
      </VSCodeButton>
    </Tooltip>
  );
}

/**
 * Make a new copy of the FoldOperation with the latest edited message for the combined preview.
 * This allows running the fold operation to use the newly typed message.
 */
export function updateFoldedMessageWithEditedMessage(): FoldOperation | undefined {
  const beingPreviewed = readAtom(operationBeingPreviewedJotai);
  if (beingPreviewed != null && beingPreviewed instanceof FoldOperation) {
    const range = beingPreviewed.getFoldRange();
    const combinedHash = getFoldRangeCommitHash(range, /* isPreview */ true);
    const [existingTitle, existingMessage] = beingPreviewed.getFoldedMessage();
    const editedMessage = readAtom(editedCommitMessages(combinedHash));

    const schema = readAtom(commitMessageFieldsSchema);
    if (schema == null) {
      return undefined;
    }

    const old = parseCommitMessageFields(schema, existingTitle, existingMessage);
    const message = editedMessage == null ? old : applyEditedFields(old, editedMessage.fields);

    const newMessage = commitMessageFieldsToString(schema, message);

    return new FoldOperation(range, newMessage);
  }
}

export function useRunFoldPreview(): [cancel: () => unknown, run: () => unknown] {
  const handlePreviewedOperation = useRunPreviewedOperation();
  const run = useCallback(() => {
    const foldOperation = updateFoldedMessageWithEditedMessage();
    if (foldOperation == null) {
      return;
    }
    handlePreviewedOperation(/* isCancel */ false, foldOperation);
    // select the optimistic commit instead of the preview commit
    writeAtom(selectedCommits, last =>
      last.size === 1 && firstOfIterable(last.values())?.startsWith(FOLD_COMMIT_PREVIEW_HASH_PREFIX)
        ? new Set([getFoldRangeCommitHash(foldOperation.getFoldRange(), /* isPreview */ false)])
        : last,
    );
  }, [handlePreviewedOperation]);
  return [
    () => {
      handlePreviewedOperation(/* isCancel */ true);
    },
    run,
  ];
}
