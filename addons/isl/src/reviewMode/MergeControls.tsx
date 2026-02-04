/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffSummary} from '../types';
import type {MergeStrategy} from '../operations/MergePROperation';

import {Button} from 'isl-components/Button';
import {Dropdown} from 'isl-components/Dropdown';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {useState, useCallback} from 'react';
import {diffSummary, allDiffSummaries} from '../codeReview/CodeReviewInfo';
import {currentPRStackContextAtom} from '../codeReview/PRStacksAtom';
import {useRunOperation, isOperationRunningAtom} from '../operationsState';
import {MergePROperation} from '../operations/MergePROperation';
import {PullStackOperation} from '../operations/PullStackOperation';
import {RebaseOperation} from '../operations/RebaseOperation';
import {CIStatusBadge} from './CIStatusBadge';
import {
  deriveMergeability,
  formatMergeBlockReasons,
  mergeInProgressAtom,
} from './mergeState';
import {writeAtom} from '../jotaiUtils';
import {showToast} from '../toast';
import {T, t} from '../i18n';
import {exitReviewMode, navigateToPRInStack} from '../reviewMode';
import {succeedableRevset} from '../types';
import './MergeControls.css';

export type MergeControlsProps = {
  prNumber: string;
};

const MERGE_STRATEGIES: {value: MergeStrategy; label: string}[] = [
  {value: 'squash', label: 'Squash and merge'},
  {value: 'merge', label: 'Create merge commit'},
  {value: 'rebase', label: 'Rebase and merge'},
];

/**
 * Type guard to check if a DiffSummary is a GitHubDiffSummary.
 */
function isGitHubDiffSummary(pr: DiffSummary): pr is DiffSummary & {type: 'github'} {
  return pr.type === 'github';
}

/**
 * Merge controls panel for review mode.
 * Shows CI status, strategy selection, and merge/rebase buttons.
 */
export function MergeControls({prNumber}: MergeControlsProps) {
  const [strategy, setStrategy] = useState<MergeStrategy>('squash');
  const [deleteBranch, setDeleteBranch] = useState(false);
  const runOperation = useRunOperation();
  const mergeInProgress = useAtomValue(mergeInProgressAtom);
  const isOperationRunning = useAtomValue(isOperationRunningAtom);
  const stackContext = useAtomValue(currentPRStackContextAtom);
  const diffs = useAtomValue(allDiffSummaries);

  // Get PR data for CI status and mergeability
  const prData = useAtomValue(diffSummary(prNumber));
  const pr = prData?.value;

  // Find the next unmerged PR in the stack (for auto-navigation after merge)
  const findNextUnmergedPR = useCallback((): {prNumber: string; headHash: string} | null => {
    if (!stackContext || stackContext.isSinglePr || !diffs.value) {
      return null;
    }
    // Stack entries are top-to-bottom, but we merge bottom-up
    // Find the first unmerged PR that isn't the current one
    // Prefer PRs closer to the base (higher index) for proper merge order
    for (let i = stackContext.entries.length - 1; i >= 0; i--) {
      const entry = stackContext.entries[i];
      if (entry.state !== 'MERGED' && entry.prNumber !== Number(prNumber)) {
        const prData = diffs.value.get(String(entry.prNumber));
        if (prData?.type === 'github' && prData.head) {
          return {prNumber: String(entry.prNumber), headHash: prData.head};
        }
      }
    }
    return null;
  }, [stackContext, diffs.value, prNumber]);

  // Check if we're currently merging this PR
  const isMerging = mergeInProgress === prNumber;

  // Check sync status - is PR behind base branch or has conflicts?
  const mergeStateStatus = pr && isGitHubDiffSummary(pr) ? pr.mergeStateStatus : undefined;
  const mergeable = pr && isGitHubDiffSummary(pr) ? pr.mergeable : undefined;
  const isBehind = mergeStateStatus === 'BEHIND';
  const hasConflicts = mergeStateStatus === 'DIRTY' || mergeable === 'CONFLICTING';
  const needsSync = isBehind || hasConflicts;

  // Handle rebase operation - uses local Sapling rebase instead of GitHub API
  // First pulls the PR if not available locally, then rebases onto main
  const handleRebase = useCallback(async () => {
    // Prefer branch name (bookmark) over hash since the hash might not exist locally
    const branchName = pr && isGitHubDiffSummary(pr) ? pr.branchName : null;

    // Use branch name if available, otherwise fall back to PR number
    const source = branchName || `pr${prNumber}`;

    // DEBUG: Log what we're doing
    // eslint-disable-next-line no-console
    console.log('[MergeControls] handleRebase called', {prNumber, branchName, source});

    if (!source) {
      showToast(t('Cannot rebase: PR branch not found'), {durationMs: 5000});
      return;
    }

    try {
      // First, pull the PR to ensure it exists locally (without --goto)
      // This uses `sl pr get` which discovers and imports the full stack
      // eslint-disable-next-line no-console
      console.log('[MergeControls] Starting PullStackOperation for PR', prNumber);
      showToast(t('Pulling PR #$pr...', {replace: {$pr: prNumber}}), {durationMs: 3000});
      await runOperation(new PullStackOperation(Number(prNumber), /* goto */ false));

      // Now rebase the PR's commit onto main using Sapling
      // eslint-disable-next-line no-console
      console.log('[MergeControls] Starting RebaseOperation with source:', source);
      showToast(t('Rebasing onto main...'), {durationMs: 3000});
      await runOperation(new RebaseOperation(
        succeedableRevset(source),
        succeedableRevset('main')
      ));

      showToast(t('Rebase complete! Push changes to update the PR.'), {durationMs: 5000});
    } catch (error) {
      // eslint-disable-next-line no-console
      console.error('[MergeControls] Rebase failed:', error);
      showToast(t('Rebase failed: $error', {replace: {$error: String(error)}}), {durationMs: 8000});
    }
  }, [pr, prNumber, runOperation]);

  // Derive mergeability
  const mergeability = pr
    ? deriveMergeability({
        signalSummary: pr.signalSummary,
        reviewDecision: isGitHubDiffSummary(pr) ? pr.reviewDecision : undefined,
        mergeable: isGitHubDiffSummary(pr) ? pr.mergeable : undefined,
        mergeStateStatus: isGitHubDiffSummary(pr) ? pr.mergeStateStatus : undefined,
        state: isGitHubDiffSummary(pr) ? pr.state : undefined,
      })
    : {canMerge: false, reasons: ['Loading PR data...']};

  // Check if this PR is the last open PR in the stack (for confetti)
  const isLastOpenPRInStack = useCallback((): boolean => {
    if (!stackContext || stackContext.isSinglePr) {
      return true; // Single PR = always "last"
    }
    const openPRs = stackContext.entries.filter(e => e.state !== 'MERGED');
    return openPRs.length <= 1;
  }, [stackContext]);

  const handleMerge = useCallback(async () => {
    if (!mergeability.canMerge || isMerging) {
      return;
    }

    writeAtom(mergeInProgressAtom, prNumber);

    try {
      const op = new MergePROperation(Number(prNumber), strategy, deleteBranch);
      await runOperation(op);

      const wasLastPR = isLastOpenPRInStack();

      if (wasLastPR) {
        // Last PR in stack merged - celebrate!
        showToast(t('Stack merged! All PRs have been merged.'), {durationMs: 5000});
        window.dispatchEvent(new CustomEvent('isl-confetti'));
        setTimeout(() => {
          exitReviewMode();
        }, 2000);
      } else {
        // Not the last PR - navigate to next one after short delay
        const nextPR = findNextUnmergedPR();
        showToast(t('PR #$pr merged successfully', {replace: {$pr: prNumber}}), {durationMs: 3000});
        if (nextPR) {
          setTimeout(() => {
            navigateToPRInStack(nextPR.prNumber, nextPR.headHash);
          }, 2000);
        }
      }
    } catch (error) {
      showToast(t('Failed to merge PR: $error', {replace: {$error: String(error)}}), {durationMs: 8000});
    } finally {
      writeAtom(mergeInProgressAtom, null);
    }
  }, [prNumber, strategy, deleteBranch, mergeability.canMerge, isMerging, runOperation, isLastOpenPRInStack, findNextUnmergedPR]);

  if (!pr) {
    return (
      <div className="merge-controls merge-controls-loading">
        <div style={{color: 'magenta', fontWeight: 'bold'}}>LOCAL DEV BUILD v2</div>
        <Icon icon="loading" /> Loading...
      </div>
    );
  }

  // Check if PR is already merged - show success state
  const prState = isGitHubDiffSummary(pr) ? pr.state : undefined;
  if (prState === 'MERGED') {
    return (
      <div className="merge-controls merge-controls-merged">
        <div className="merge-success-message">
          <Icon icon="check" />
          <span><T>This PR has been merged!</T></span>
        </div>
      </div>
    );
  }

  // Get CI checks - only available on GitHub PRs
  const ciChecks = isGitHubDiffSummary(pr) ? pr.ciChecks : undefined;

  // Check if this is a stacked PR that needs the base PR merged first
  const isStackedPRNeedingBaseMerge = useCallback((): boolean => {
    if (!stackContext || stackContext.isSinglePr || !diffs.value) {
      return false;
    }
    const currentIndex = stackContext.entries.findIndex(e => e.prNumber === Number(prNumber));
    if (currentIndex < 0) {
      return false;
    }
    // Check if there are unmerged PRs below this one (closer to base)
    for (let i = currentIndex + 1; i < stackContext.entries.length; i++) {
      const entry = stackContext.entries[i];
      const prData = diffs.value.get(String(entry.prNumber));
      // If PR is in diffs and not merged, it needs to be merged first
      if (prData && prData.state !== 'MERGED') {
        return true;
      }
    }
    return false;
  }, [stackContext, prNumber, diffs.value]);

  const needsBasePRMerged = isStackedPRNeedingBaseMerge();

  // If behind base branch or has conflicts, show sync UI
  if (needsSync) {
    const isStackOrderIssue = needsBasePRMerged && hasConflicts;

    // Find the base PR to navigate to
    const findBasePR = (): {prNumber: string; headHash: string} | null => {
      if (!stackContext || !diffs.value) return null;
      const currentIndex = stackContext.entries.findIndex(e => e.prNumber === Number(prNumber));
      if (currentIndex < 0) return null;
      for (let i = currentIndex + 1; i < stackContext.entries.length; i++) {
        const entry = stackContext.entries[i];
        if (entry.state !== 'MERGED') {
          const prData = diffs.value.get(String(entry.prNumber));
          if (prData?.type === 'github' && prData.head) {
            return {prNumber: String(entry.prNumber), headHash: prData.head};
          }
        }
      }
      return null;
    };

    const handleGoToBase = () => {
      const basePR = findBasePR();
      if (basePR) {
        navigateToPRInStack(basePR.prNumber, basePR.headHash);
      }
    };

    if (isStackOrderIssue) {
      return (
        <div className="merge-controls merge-controls-sync merge-controls-stack-order">
          <div className="merge-controls-sync-message">
            <Icon icon="info" />
            <span><T>Merge the base PR first - stacked PRs must be merged bottom-up</T></span>
          </div>
          <Tooltip title={t('Navigate to the base PR that needs to be merged first')} placement="bottom">
            <Button primary onClick={handleGoToBase}>
              <Icon icon="arrow-down" slot="start" />
              <T>Go to base PR</T>
            </Button>
          </Tooltip>
        </div>
      );
    }

    // Conflicts or behind - show rebase button
    const isConflicts = hasConflicts;
    return (
      <div className={`merge-controls merge-controls-sync ${isConflicts ? 'merge-controls-conflicts' : ''}`}>
        <div className="merge-controls-sync-message">
          <Icon icon="warning" />
          <span>
            {isConflicts
              ? <T>This branch has conflicts - rebase to resolve</T>
              : <T>This branch is out of date with the base branch</T>}
          </span>
        </div>
        <Tooltip title={t('Rebase PR commits onto latest main using Sapling')} placement="bottom">
          <Button
            primary
            disabled={isOperationRunning}
            onClick={handleRebase}>
            {isOperationRunning ? (
              <>
                <Icon icon="loading" slot="start" />
                <T>Rebasing...</T>
              </>
            ) : (
              <>
                <Icon icon="repo-sync" slot="start" />
                <T>Rebase onto main</T>
              </>
            )}
          </Button>
        </Tooltip>
      </div>
    );
  }

  return (
    <div className="merge-controls">
      <div className="merge-controls-row">
        <div className="merge-controls-status">
          <CIStatusBadge
            signalSummary={pr.signalSummary}
            ciChecks={ciChecks}
          />
        </div>

        <div className="merge-controls-actions">
          <div className="merge-strategy-group">
            <div className="merge-strategy-row">
              <div className="merge-strategy-select">
                <Dropdown
                  options={MERGE_STRATEGIES.map(({value, label}) => ({value, name: label}))}
                  value={strategy}
                  onChange={(e) => setStrategy(e.currentTarget.value as MergeStrategy)}
                  disabled={isMerging}
                />
              </div>
              <Tooltip
                title={
                  mergeability.canMerge
                    ? t('Merge PR #$pr', {replace: {$pr: prNumber}})
                    : formatMergeBlockReasons(mergeability.reasons)
                }
                placement="bottom">
                <Button
                  className="merge-btn"
                  disabled={!mergeability.canMerge || isMerging}
                  onClick={handleMerge}>
                  {isMerging ? (
                    <>
                      <Icon icon="loading" slot="start" />
                      <T>Merging...</T>
                    </>
                  ) : (
                    <>
                      <Icon icon="git-merge" slot="start" />
                      <T>Merge</T>
                    </>
                  )}
                </Button>
              </Tooltip>
            </div>
            <label className="merge-delete-branch">
              <input
                type="checkbox"
                checked={deleteBranch}
                onChange={(e) => setDeleteBranch(e.target.checked)}
                disabled={isMerging}
              />
              <T>Delete branch</T>
            </label>
          </div>
        </div>
      </div>

      {!mergeability.canMerge && (
        <div className="merge-block-reasons">
          {mergeability.reasons.map((reason, i) => (
            <div key={i} className="merge-block-reason">
              <Icon icon="warning" size="S" />
              {reason}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
