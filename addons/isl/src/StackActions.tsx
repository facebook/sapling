/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DagCommitInfo} from './dag/dag';
import type {Hash} from './types';

import {globalRecoil} from './AccessGlobalRecoil';
import {CleanupButton, isStackEligibleForCleanup} from './Cleanup';
import {FlexRow} from './ComponentUtils';
import {shouldShowSubmitStackConfirmation, useShowConfirmSubmitStack} from './ConfirmSubmitStack';
import {HighlightCommitsWhileHovering} from './HighlightedCommits';
import {OperationDisabledButton} from './OperationDisabledButton';
import {showSuggestedRebaseForStack, SuggestedRebaseButton} from './SuggestedRebase';
import {Tooltip, DOCUMENTATION_DELAY} from './Tooltip';
import {codeReviewProvider, allDiffSummaries} from './codeReview/CodeReviewInfo';
import {SyncStatus, syncStatusAtom} from './codeReview/syncStatus';
import {T, t} from './i18n';
import {IconStack} from './icons/IconStack';
import {dagWithPreviews} from './previews';
import {useRunOperation, latestUncommittedChangesData} from './serverAPIState';
import {useConfirmUnsavedEditsBeforeSplit} from './stackEdit/ui/ConfirmUnsavedEditsBeforeSplit';
import {StackEditIcon} from './stackEdit/ui/StackEditIcon';
import {editingStackIntentionHashes, loadingStackState} from './stackEdit/ui/stackEditState';
import {succeedableRevset} from './types';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue, useRecoilState} from 'recoil';
import {type ContextMenuItem, useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';

import './StackActions.css';

/**
 * Actions at the bottom of a stack of commits that acts on the whole stack,
 * like submitting, hiding, editing the stack.
 */
export function StackActions({hash}: {hash: Hash}): React.ReactElement | null {
  const reviewProvider = useRecoilValue(codeReviewProvider);
  const diffMap = useRecoilValue(allDiffSummaries);
  const stackHashes = useRecoilValue(editingStackIntentionHashes)[1];
  const loadingState = useRecoilValue(loadingStackState);
  const suggestedRebase = useRecoilValue(showSuggestedRebaseForStack(hash));
  const dag = useRecoilValue(dagWithPreviews);
  const runOperation = useRunOperation();
  const syncStatusMap = useRecoilValue(syncStatusAtom);

  // buttons at the bottom of the stack
  const actions = [];
  // additional actions hidden behind [...] menu.
  // Non-empty only when actions is non-empty.
  const moreActions: Array<ContextMenuItem> = [];
  const confirmShouldSubmit = useShowConfirmSubmitStack();
  const contextMenu = useContextMenu(() => moreActions);

  const isStackEditingActivated =
    stackHashes.size > 0 &&
    loadingState.state === 'hasValue' &&
    dag
      .descendants(hash)
      .toSeq()
      .some(h => stackHashes.has(h));

  const showCleanupButton =
    reviewProvider == null || diffMap?.value == null
      ? false
      : isStackEligibleForCleanup(hash, dag, diffMap.value, reviewProvider);

  const info = dag.get(hash);

  if (info == null) {
    return null;
  }

  if (reviewProvider !== null && !isStackEditingActivated) {
    const reviewActions =
      diffMap.value == null
        ? {}
        : reviewProvider?.getSupportedStackActions(hash, dag, diffMap.value);
    const resubmittableStack = reviewActions?.resubmittableStack;
    const submittableStack = reviewActions?.submittableStack;
    const MIN_STACK_SIZE_TO_SUGGEST_SUBMIT = 2; // don't show "submit stack" on single commits... they're not really "stacks".

    const locallyChangedCommits = resubmittableStack?.filter(
      c => syncStatusMap?.get(c.hash) === SyncStatus.LocalIsNewer,
    );

    const willShowConfirmationModal = shouldShowSubmitStackConfirmation(
      globalRecoil().getSnapshot(),
    );

    // any existing diffs -> show resubmit stack,
    if (
      resubmittableStack != null &&
      resubmittableStack.length >= MIN_STACK_SIZE_TO_SUGGEST_SUBMIT
    ) {
      const TooltipContent = () => {
        return (
          <div className="resubmit-stack-tooltip">
            <T replace={{$cmd: reviewProvider?.submitCommandName()}}>
              Submit new version of commits in this stack for review with $cmd.
            </T>
            {willShowConfirmationModal && (
              <div>
                <T>Draft mode and update message can be configured before submitting.</T>
              </div>
            )}
            {locallyChangedCommits != null && locallyChangedCommits.length > 0 && (
              <div>
                <Icon icon="circle-filled" color={'blue'} />
                <T count={locallyChangedCommits.length}>someCommitsUpdatedLocallyAddendum</T>
              </div>
            )}
          </div>
        );
      };
      let icon = <Icon icon="cloud-upload" slot="start" />;
      if (locallyChangedCommits != null && locallyChangedCommits.length > 0) {
        icon = (
          <IconStack slot="start">
            <Icon icon="cloud-upload" />
            <Icon icon="circle-large-filled" color={'blue'} />
          </IconStack>
        );
      }
      actions.push(
        <Tooltip key="resubmit-stack" component={() => <TooltipContent />} placement="bottom">
          <HighlightCommitsWhileHovering toHighlight={resubmittableStack}>
            <OperationDisabledButton
              // Use the diffId in the key so that only this "resubmit stack" button shows the spinner.
              contextKey={`resubmit-stack-on-${info.diffId}`}
              appearance="icon"
              icon={icon}
              runOperation={async () => {
                const confirmation = await confirmShouldSubmit('resubmit', resubmittableStack);
                if (!confirmation) {
                  return [];
                }
                return reviewProvider.submitOperation(resubmittableStack, {
                  draft: confirmation.submitAsDraft,
                  updateMessage: confirmation.updateMessage,
                });
              }}>
              <T>Resubmit stack</T>
            </OperationDisabledButton>
          </HighlightCommitsWhileHovering>
        </Tooltip>,
      );
      // any non-submitted diffs -> "submit all commits this stack" in hidden group
      if (
        submittableStack != null &&
        submittableStack.length > 0 &&
        submittableStack.length > resubmittableStack.length
      ) {
        moreActions.push({
          label: (
            <HighlightCommitsWhileHovering key="submit-entire-stack" toHighlight={submittableStack}>
              <FlexRow>
                <Icon icon="cloud-upload" slot="start" />
                <T>Submit entire stack</T>
              </FlexRow>
            </HighlightCommitsWhileHovering>
          ),
          onClick: async () => {
            const confirmation = await confirmShouldSubmit('submit-all', submittableStack);
            if (!confirmation) {
              return [];
            }
            runOperation(
              reviewProvider.submitOperation(submittableStack, {
                draft: confirmation.submitAsDraft,
                updateMessage: confirmation.updateMessage,
              }),
            );
          },
        });
      }
      // NO non-submitted diffs -> nothing in hidden group
    } else if (
      submittableStack != null &&
      submittableStack.length >= MIN_STACK_SIZE_TO_SUGGEST_SUBMIT
    ) {
      // We need to associate this operation with the stack we're submitting,
      // but during submitting, we'll amend the original commit, so hash is not accurate.
      // Parent is close, but if you had multiple stacks rebased to the same public commit,
      // all those stacks would render the same key and show the same spinner.
      // So parent hash + title heuristic lets us almost always show the spinner for only this stack.
      const contextKey = `submit-stack-on-${info.parents.at(0)}-${info.title.replace(/ /g, '_')}`;

      const tooltip = t(
        willShowConfirmationModal
          ? 'Submit commits in this stack for review with $cmd.\n\nDraft mode and update message can be configured before submitting.'
          : 'Submit commits in this stack for review with $cmd.',
        {replace: {$cmd: reviewProvider?.submitCommandName()}},
      );
      // NO existing diffs -> show submit stack ()
      actions.push(
        <Tooltip key="submit-stack" title={tooltip} placement="bottom">
          <HighlightCommitsWhileHovering toHighlight={submittableStack}>
            <OperationDisabledButton
              contextKey={contextKey}
              appearance="icon"
              icon={<Icon icon="cloud-upload" slot="start" />}
              runOperation={async () => {
                const allCommits = submittableStack;
                const confirmation = await confirmShouldSubmit('submit', allCommits);
                if (!confirmation) {
                  return [];
                }
                return reviewProvider.submitOperation(submittableStack, {
                  draft: confirmation.submitAsDraft,
                  updateMessage: confirmation.updateMessage,
                });
              }}>
              <T>Submit stack</T>
            </OperationDisabledButton>
          </HighlightCommitsWhileHovering>
        </Tooltip>,
      );
    }
  }

  const hasChildren = dag.childHashes(hash).size > 0;
  if (hasChildren) {
    actions.push(<StackEditButton key="edit-stack" info={info} />);
  }

  if (showCleanupButton) {
    actions.push(<CleanupButton key="cleanup" commit={info} hasChildren={hasChildren} />);
    // cleanup button implies no need to rebase this stack
  } else if (suggestedRebase) {
    actions.push(<SuggestedRebaseButton key="suggested-rebase" source={succeedableRevset(hash)} />);
  }

  if (actions.length === 0) {
    return null;
  }
  const moreActionsButton =
    moreActions.length === 0 ? null : (
      <VSCodeButton key="more-actions" appearance="icon" onClick={contextMenu}>
        <Icon icon="ellipsis" />
      </VSCodeButton>
    );
  return (
    <div className="commit-tree-stack-actions" data-testid="commit-tree-stack-actions">
      {actions}
      {moreActionsButton}
    </div>
  );
}

function StackEditButton({info}: {info: DagCommitInfo}): React.ReactElement | null {
  const uncommitted = useRecoilValue(latestUncommittedChangesData);
  const dag = useRecoilValue(dagWithPreviews);
  const [[, stackHashes], setStackIntentionHashes] = useRecoilState(editingStackIntentionHashes);
  const loadingState = useRecoilValue(loadingStackState);

  const set = dag.descendants(info.hash);
  const stackCommits = dag.getBatch(set.toArray());
  const isEditing = stackHashes.size > 0 && set.toSeq().some(h => stackHashes.has(h));

  const isPreview = info.previewType != null;
  const isLoading = isEditing && loadingState.state === 'loading';
  const isError = isEditing && loadingState.state === 'hasError';
  const isLinear = dag.merge(set).size === 0 && dag.heads(set).size === 1;
  const isDirty = stackCommits.some(c => c.isHead) && uncommitted.files.length > 0;
  const hasPublic = stackCommits.some(c => c.phase === 'public');
  const obsoleted = stackCommits.filter(c => c.successorInfo != null);
  const hasObsoleted = obsoleted.length > 0;
  const disabled =
    isDirty || hasObsoleted || !isLinear || isLoading || isError || isPreview || hasPublic;
  const title = isError
    ? t(`Failed to load stack: ${loadingState.error}`)
    : isLoading
    ? loadingState.exportedStack === undefined
      ? t('Reading stack content')
      : t('Analyzing stack content')
    : hasObsoleted
    ? t('Cannot edit stack with commits that have newer versions')
    : isDirty
    ? t(
        'Cannot edit stack when there are uncommitted changes.\nCommit or amend your changes first.',
      )
    : isPreview
    ? t('Cannot edit pending changes')
    : hasPublic
    ? t('Cannot edit public commits')
    : isLinear
    ? t('Reorder, fold, or drop commits')
    : t('Cannot edit non-linear stack');
  const highlight = disabled ? [] : stackCommits;
  const tooltipDelay = disabled && !isLoading ? undefined : DOCUMENTATION_DELAY;
  const icon = isLoading ? <Icon icon="loading" slot="start" /> : <StackEditIcon slot="start" />;
  const confirmUnsavedEditsBeforeSplit = useConfirmUnsavedEditsBeforeSplit();

  return (
    <HighlightCommitsWhileHovering key="submit-stack" toHighlight={highlight}>
      <Tooltip title={title} delayMs={tooltipDelay} placement="bottom">
        <VSCodeButton
          className={`edit-stack-button ${disabled && 'disabled'}`}
          disabled={disabled}
          appearance="icon"
          onClick={async () => {
            if (!(await confirmUnsavedEditsBeforeSplit(stackCommits, 'edit_stack'))) {
              return;
            }
            setStackIntentionHashes(['general', new Set<Hash>(set)]);
          }}>
          {icon}
          <T>Edit stack</T>
        </VSCodeButton>
      </Tooltip>
    </HighlightCommitsWhileHovering>
  );
}
