/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UICodeReviewProvider} from './codeReview/UICodeReviewProvider';
import type {DiffSummary, CommitInfo, Hash} from './types';

import {FlexRow} from './ComponentUtils';
import {useShowConfirmSubmitStack} from './ConfirmSubmitStack';
import {HighlightCommitsWhileHovering} from './HighlightedCommits';
import {OperationDisabledButton} from './OperationDisabledButton';
import {showSuggestedRebaseForStack, SuggestedRebaseButton} from './SuggestedRebase';
import {Tooltip, DOCUMENTATION_DELAY} from './Tooltip';
import {codeReviewProvider, allDiffSummaries} from './codeReview/CodeReviewInfo';
import {type CommitTreeWithPreviews, walkTreePostorder, isTreeLinear} from './getCommitTree';
import {T, t} from './i18n';
import {HideOperation} from './operations/HideOperation';
import {useRunOperation, latestUncommittedChangesData} from './serverAPIState';
import {StackEditIcon} from './stackEdit/ui/StackEditIcon';
import {editingStackIntentionHashes, loadingStackState} from './stackEdit/ui/stackEditState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue, useRecoilState} from 'recoil';
import {type ContextMenuItem, useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {generatorContains, unwrap} from 'shared/utils';

/**
 * Actions at the bottom of a stack of commits that acts on the whole stack,
 * like submitting, hiding, editing the stack.
 */
export function StackActions({tree}: {tree: CommitTreeWithPreviews}): React.ReactElement | null {
  const reviewProvider = useRecoilValue(codeReviewProvider);
  const diffMap = useRecoilValue(allDiffSummaries);
  const stackHashes = useRecoilValue(editingStackIntentionHashes)[1];
  const loadingState = useRecoilValue(loadingStackState);
  const suggestedRebase = useRecoilValue(showSuggestedRebaseForStack(tree.info.hash));
  const runOperation = useRunOperation();

  // buttons at the bottom of the stack
  const actions = [];
  // additional actions hidden behind [...] menu.
  // Non-empty only when actions is non-empty.
  const moreActions: Array<ContextMenuItem> = [];

  const isStackEditingActivated =
    stackHashes.size > 0 &&
    loadingState.state === 'hasValue' &&
    generatorContains(walkTreePostorder([tree]), v => stackHashes.has(v.info.hash));

  const showCleanupButton =
    reviewProvider == null || diffMap?.value == null
      ? false
      : isStackEligibleForCleanup(tree, diffMap.value, reviewProvider);

  const confirmShouldSubmit = useShowConfirmSubmitStack();
  const contextMenu = useContextMenu(() => moreActions);
  if (reviewProvider !== null && !isStackEditingActivated) {
    const reviewActions =
      diffMap.value == null ? {} : reviewProvider?.getSupportedStackActions(tree, diffMap.value);
    const resubmittableStack = reviewActions?.resubmittableStack;
    const submittableStack = reviewActions?.submittableStack;
    const MIN_STACK_SIZE_TO_SUGGEST_SUBMIT = 2; // don't show "submit stack" on single commits... they're not really "stacks".

    // any existing diffs -> show resubmit stack,
    if (
      resubmittableStack != null &&
      resubmittableStack.length >= MIN_STACK_SIZE_TO_SUGGEST_SUBMIT
    ) {
      actions.push(
        <HighlightCommitsWhileHovering key="resubmit-stack" toHighlight={resubmittableStack}>
          <OperationDisabledButton
            // Use the diffId in the key so that only this "resubmit stack" button shows the spinner.
            contextKey={`resubmit-stack-on-${tree.info.diffId}`}
            appearance="icon"
            icon={<Icon icon="cloud-upload" slot="start" />}
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
        </HighlightCommitsWhileHovering>,
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
      const contextKey = `submit-stack-on-${tree.info.parents[0]}-${tree.info.title.replace(
        / /g,
        '_',
      )}`;
      // NO existing diffs -> show submit stack ()
      actions.push(
        <HighlightCommitsWhileHovering key="submit-stack" toHighlight={submittableStack}>
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
        </HighlightCommitsWhileHovering>,
      );
    }
  }

  if (tree.children.length > 0) {
    actions.push(<StackEditButton key="edit-stack" tree={tree} />);
  }

  if (showCleanupButton) {
    actions.push(
      <CleanupButton key="cleanup" commit={tree.info} hasChildren={tree.children.length > 0} />,
    );
    // cleanup button implies no need to rebase this stack
  } else if (suggestedRebase) {
    actions.push(<SuggestedRebaseButton key="suggested-rebase" stackBaseHash={tree.info.hash} />);
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

function isStackEligibleForCleanup(
  tree: CommitTreeWithPreviews,
  diffMap: Map<string, DiffSummary>,
  provider: UICodeReviewProvider,
): boolean {
  if (
    tree.info.diffId == null ||
    tree.info.isHead || // don't allow hiding a stack you're checked out on
    diffMap.get(tree.info.diffId) == null ||
    !provider.isDiffEligibleForCleanup(unwrap(diffMap.get(tree.info.diffId)))
  ) {
    return false;
  }

  // any child not eligible -> don't show
  for (const subtree of tree.children) {
    if (!isStackEligibleForCleanup(subtree, diffMap, provider)) {
      return false;
    }
  }

  return true;
}

function CleanupButton({commit, hasChildren}: {commit: CommitInfo; hasChildren: boolean}) {
  const runOperation = useRunOperation();
  return (
    <Tooltip
      title={
        hasChildren
          ? t('You can safely "clean up" by hiding this stack of commits.')
          : t('You can safely "clean up" by hiding this commit.')
      }
      placement="bottom">
      <VSCodeButton
        appearance="icon"
        onClick={() => {
          runOperation(new HideOperation(commit.hash));
        }}>
        <Icon icon="eye-closed" slot="start" />
        {hasChildren ? <T>Clean up stack</T> : <T>Clean up</T>}
      </VSCodeButton>
    </Tooltip>
  );
}

function StackEditButton({tree}: {tree: CommitTreeWithPreviews}): React.ReactElement | null {
  const uncommitted = useRecoilValue(latestUncommittedChangesData);
  const [[, stackHashes], setStackIntentionHashes] = useRecoilState(editingStackIntentionHashes);
  const loadingState = useRecoilValue(loadingStackState);

  const stackCommits = [...walkTreePostorder([tree])].map(t => t.info);
  const isEditing = stackHashes.size > 0 && stackCommits.some(c => stackHashes.has(c.hash));

  const isPreview = tree.previewType != null;
  const isLoading = isEditing && loadingState.state === 'loading';
  const isError = isEditing && loadingState.state === 'hasError';
  const isLinear = isTreeLinear(tree);
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

  return (
    <HighlightCommitsWhileHovering key="submit-stack" toHighlight={highlight}>
      <Tooltip title={title} delayMs={tooltipDelay} placement="bottom">
        <VSCodeButton
          className={`edit-stack-button ${disabled && 'disabled'}`}
          disabled={disabled}
          appearance="icon"
          onClick={() => {
            setStackIntentionHashes(['general', new Set<Hash>(stackCommits.map(c => c.hash))]);
          }}>
          {icon}
          <T>Edit stack</T>
        </VSCodeButton>
      </Tooltip>
    </HighlightCommitsWhileHovering>
  );
}
