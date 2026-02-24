/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffSummary} from '../types';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {useState, useCallback} from 'react';
import {diffSummary, allDiffSummaries, triggerFullDiffSummariesRefresh} from '../codeReview/CodeReviewInfo';
import {currentPRStackContextAtom, prStacksAtom} from '../codeReview/PRStacksAtom';
import {useRunOperation} from '../operationsState';
import {MergePROperation} from '../operations/MergePROperation';
import {ClosePROperation} from '../operations/ClosePROperation';
import {SyncPROperation} from '../operations/SyncPROperation';
import {
  deriveMergeability,
  formatMergeBlockReasons,
  mergeInProgressAtom,
} from './mergeState';
import {writeAtom} from '../jotaiUtils';
import {showToast} from '../toast';
import {T, t} from '../i18n';
import {exitReviewMode} from '../reviewMode';
import serverAPI from '../ClientToServerAPI';
import './MergeControls.css';

export type MergeControlsProps = {
  prNumber: string;
};

/**
 * Type guard to check if a DiffSummary is a GitHubDiffSummary.
 */
function isGitHubDiffSummary(pr: DiffSummary): pr is DiffSummary & {type: 'github'; url: string} {
  return pr.type === 'github';
}

/**
 * Merge controls panel for review mode.
 *
 * Key behaviors:
 * - Always uses rebase merge strategy
 * - Can merge from any PR in the stack (top is typical, but middle works too)
 * - After merge, closes all PRs below (their changes are already in main)
 * - If conflicts exist, shows link to GitHub to resolve (no local rebase)
 */
export function MergeControls({prNumber}: MergeControlsProps) {
  const [deleteBranch, setDeleteBranch] = useState(true);
  const [isSyncing, setIsSyncing] = useState(false);
  const runOperation = useRunOperation();
  const mergeInProgress = useAtomValue(mergeInProgressAtom);
  const stackContext = useAtomValue(currentPRStackContextAtom);
  const diffs = useAtomValue(allDiffSummaries);
  const stacks = useAtomValue(prStacksAtom);

  // Get PR data for CI status and mergeability
  const prData = useAtomValue(diffSummary(prNumber));
  const pr = prData?.value;

  // Check if we're currently merging this PR
  const isMerging = mergeInProgress === prNumber;

  // Check if this PR is part of a stale stack (top PR was merged via GitHub)
  const currentStack = stacks.find(s => s.prs.some(p => String(p.number) === prNumber));
  const isStaleStack = currentStack?.hasStaleAbove ?? false;
  const mergedAbovePrNumber = currentStack?.mergedAbovePrNumber;

  // Check sync status - has conflicts?
  const mergeable = pr && isGitHubDiffSummary(pr) ? pr.mergeable : undefined;
  const mergeStateStatus = pr && isGitHubDiffSummary(pr) ? pr.mergeStateStatus : undefined;
  const hasConflicts = mergeStateStatus === 'DIRTY' || mergeable === 'CONFLICTING';

  // Check if PR is a draft (use pr.state which is always accurate, unlike mergeStateStatus)
  const prState = pr && isGitHubDiffSummary(pr) ? pr.state : undefined;
  const isDraft = prState === 'DRAFT';

  // Get PR URL for GitHub link
  const prUrl = pr && isGitHubDiffSummary(pr) ? pr.url : null;
  const prNodeId = pr && isGitHubDiffSummary(pr) ? pr.nodeId : null;

  // Get PRs below this one in the stack (to close after merge)
  const getPRsBelowInStack = useCallback((): number[] => {
    if (!stackContext || stackContext.isSinglePr || !diffs.value) {
      return [];
    }
    const currentIndex = stackContext.entries.findIndex(e => e.prNumber === Number(prNumber));
    if (currentIndex < 0) {
      return [];
    }
    // Get all unmerged PRs below this one (higher index = closer to base)
    const prsBelow: number[] = [];
    for (let i = currentIndex + 1; i < stackContext.entries.length; i++) {
      const entry = stackContext.entries[i];
      const prData = diffs.value.get(String(entry.prNumber));
      if (prData && prData.state !== 'MERGED' && prData.state !== 'CLOSED') {
        prsBelow.push(entry.prNumber);
      }
    }
    return prsBelow;
  }, [stackContext, prNumber, diffs.value]);

  // Check if branch is behind base branch
  const isBehind = mergeStateStatus === 'BEHIND';

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

  // Filter out "behind" and "draft" reasons from the general list since we handle them with dedicated UI
  const filteredReasons = mergeability.reasons.filter(r => !r.includes('behind') && !r.includes('draft'));
  const canMerge = filteredReasons.length === 0 && !hasConflicts && !isBehind && !isDraft;

  const handleMerge = useCallback(async () => {
    if (!canMerge || isMerging) {
      return;
    }

    writeAtom(mergeInProgressAtom, prNumber);

    try {
      // Always use rebase merge strategy
      const op = new MergePROperation(Number(prNumber), 'rebase', deleteBranch);
      await runOperation(op, /* throwOnError */ true);

      // Get PRs below to close
      const prsBelow = getPRsBelowInStack();

      if (prsBelow.length > 0) {
        // Close all PRs below - their changes are already in main via the merged PR
        showToast(
          t('Closing $count PRs below (already merged)...', {replace: {$count: String(prsBelow.length)}}),
          {durationMs: 3000}
        );

        for (const belowPrNumber of prsBelow) {
          try {
            const closeOp = new ClosePROperation(
              belowPrNumber,
              `Closed automatically - changes were included in PR #${prNumber} which was merged.`
            );
            await runOperation(closeOp, /* throwOnError */ true);
          } catch (err) {
            // Log but don't fail the whole operation if one close fails
            // eslint-disable-next-line no-console
            console.warn(`Failed to close PR #${belowPrNumber}:`, err);
          }
        }
      }

      // Success - refresh PR list (full replace so merged PR disappears), celebrate and exit
      triggerFullDiffSummariesRefresh();
      showToast(t('PR #$pr merged successfully!', {replace: {$pr: prNumber}}), {durationMs: 5000});
      window.dispatchEvent(new CustomEvent('isl-confetti'));
      setTimeout(() => {
        exitReviewMode();
      }, 2000);
    } catch (error) {
      showToast(t('Failed to merge PR: $error', {replace: {$error: String(error)}}), {durationMs: 8000});
    } finally {
      writeAtom(mergeInProgressAtom, null);
    }
  }, [prNumber, deleteBranch, canMerge, isMerging, runOperation, getPRsBelowInStack]);

  const handleUpdateBranch = useCallback(async () => {
    if (isSyncing) {
      return;
    }
    setIsSyncing(true);
    try {
      const syncOp = new SyncPROperation(prNumber);
      await runOperation(syncOp, /* throwOnError */ true);
      showToast(t('Branch updated successfully'), {durationMs: 3000});
      triggerFullDiffSummariesRefresh();
    } catch (error) {
      showToast(t('Failed to update branch: $error', {replace: {$error: String(error)}}), {durationMs: 8000});
    } finally {
      setIsSyncing(false);
    }
  }, [prNumber, isSyncing, runOperation]);

  const [isPublishing, setIsPublishing] = useState(false);
  const handlePublish = useCallback(async () => {
    if (isPublishing || !prNodeId) {
      return;
    }
    setIsPublishing(true);
    try {
      serverAPI.postMessage({
        type: 'publishPullRequest',
        pullRequestId: prNodeId,
      });
      const response = await serverAPI.nextMessageMatching(
        'publishedPullRequest',
        () => true,
      );
      if (response.result.error) {
        showToast(t('Failed to publish PR: $error', {replace: {$error: response.result.error.message}}), {durationMs: 8000});
      } else {
        showToast(t('PR published and ready for review'), {durationMs: 3000});
        triggerFullDiffSummariesRefresh();
      }
    } catch (error) {
      showToast(t('Failed to publish PR: $error', {replace: {$error: String(error)}}), {durationMs: 8000});
    } finally {
      setIsPublishing(false);
    }
  }, [prNodeId, isPublishing]);

  if (!pr) {
    return (
      <div className="merge-controls merge-controls-loading">
        <Icon icon="loading" /> Loading...
      </div>
    );
  }

  // Check if PR is already merged - show success state
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

  // If this PR is part of a stale stack (top was merged via GitHub), show close button
  // Get all stale PRs in this stack (open PRs that should be closed)
  const stalePRsInStack = currentStack?.prs.filter(
    p => p.state !== 'MERGED' && p.state !== 'CLOSED'
  ) ?? [];
  const stalePRCount = stalePRsInStack.length;

  if (isStaleStack) {
    const handleCloseAllStale = async () => {
      writeAtom(mergeInProgressAtom, prNumber);
      try {
        for (const stalePR of stalePRsInStack) {
          const closeOp = new ClosePROperation(
            Number(stalePR.number),
            `Closed: changes already merged via PR #${mergedAbovePrNumber}`
          );
          await runOperation(closeOp, /* throwOnError */ true);
        }
        showToast(t('Closed $count stale PRs', {replace: {$count: String(stalePRCount)}}), {durationMs: 3000});
        exitReviewMode();
        // Refresh the PR list after a short delay to let GitHub propagate the changes
        // Use full refresh to replace (not merge) so closed PRs disappear
        setTimeout(() => {
          triggerFullDiffSummariesRefresh();
        }, 1500);
      } catch (error) {
        showToast(t('Failed to close PRs: $error', {replace: {$error: String(error)}}), {durationMs: 5000});
      } finally {
        writeAtom(mergeInProgressAtom, null);
      }
    };

    return (
      <div className="merge-controls merge-controls-stale">
        <div className="merge-stale-message">
          <Icon icon="info" />
          <span>
            <T replace={{$pr: mergedAbovePrNumber ?? '?'}}>
              This PR is stale — its changes were already merged via PR #$pr on GitHub.
            </T>
          </span>
        </div>
        <div className="merge-stale-explanation">
          <T>This happens when someone merges directly on GitHub instead of through ISL. You can safely close this PR.</T>
        </div>
        <Button
          className="close-stale-btn"
          onClick={handleCloseAllStale}
          disabled={isMerging}>
          {isMerging ? (
            <>
              <Icon icon="loading" slot="start" />
              <T>Closing...</T>
            </>
          ) : (
            <>
              <Icon icon="trash" slot="start" />
              <span>Close {stalePRCount} stale PR{stalePRCount !== 1 ? 's' : ''}</span>
            </>
          )}
        </Button>
      </div>
    );
  }

  // If conflicts exist, show link to GitHub to resolve
  if (hasConflicts) {
    return (
      <div className="merge-controls">
        <div className="merge-controls-row">
          <div className="merge-controls-actions">
            <div className="merge-strategy-group">
              <div className="merge-sync-status merge-sync-conflicts">
                <Icon icon="warning" />
                <span><T>Merge conflicts detected</T></span>
              </div>
              <div className="merge-strategy-row">
                {prUrl && (
                  <Tooltip title={t('Open GitHub to resolve conflicts')} placement="top">
                    <Button
                      className="resolve-conflicts-btn"
                      onClick={() => window.open(prUrl, '_blank')}>
                      <Icon icon="link-external" slot="start" />
                      <T>Resolve on GitHub</T>
                    </Button>
                  </Tooltip>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>
    );
  }

  // If PR is a draft, show publish button instead of merge
  if (isDraft) {
    return (
      <div className="merge-controls">
        <div className="merge-controls-row">
          <div className="merge-controls-actions">
            <div className="merge-strategy-group">
              <div className="merge-sync-status merge-sync-draft">
                <Icon icon="edit" />
                <span><T>This pull request is still a draft</T></span>
              </div>
              <div className="merge-strategy-row">
                <Tooltip title={t('Mark this PR as ready for review')} placement="top">
                  <Button
                    className="publish-pr-btn"
                    disabled={isPublishing || !prNodeId}
                    onClick={handlePublish}>
                    {isPublishing ? (
                      <>
                        <Icon icon="loading" slot="start" />
                        <T>Publishing...</T>
                      </>
                    ) : (
                      <>
                        <Icon icon="cloud-upload" slot="start" />
                        <T>Ready for review</T>
                      </>
                    )}
                  </Button>
                </Tooltip>
              </div>
            </div>
          </div>
        </div>
      </div>
    );
  }

  // If branch is behind base, show update branch button instead of merge
  if (isBehind) {
    return (
      <div className="merge-controls">
        <div className="merge-controls-row">
          <div className="merge-controls-actions">
            <div className="merge-strategy-group">
              <div className="merge-sync-status merge-sync-behind">
                <Icon icon="warning" />
                <span><T>This branch is out of date with the base branch</T></span>
              </div>
              <div className="merge-strategy-row">
                <Tooltip title={t('Update this branch by rebasing onto the base branch')} placement="top">
                  <Button
                    className="update-branch-btn"
                    disabled={isSyncing}
                    onClick={handleUpdateBranch}>
                    {isSyncing ? (
                      <>
                        <Icon icon="loading" slot="start" />
                        <T>Updating...</T>
                      </>
                    ) : (
                      <>
                        <Icon icon="sync" slot="start" />
                        <T>Update branch</T>
                      </>
                    )}
                  </Button>
                </Tooltip>
              </div>
            </div>
          </div>
        </div>
        {filteredReasons.length > 0 && (
          <div className="merge-block-reasons">
            {filteredReasons.map((reason, i) => (
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

  // Count PRs that will be closed after merge
  const prsBelow = getPRsBelowInStack();

  return (
    <div className="merge-controls">
      <div className="merge-controls-row">
        <div className="merge-controls-actions">
          <div className="merge-strategy-group">
            <div className="merge-strategy-row">
              <Tooltip
                title={
                  canMerge
                    ? t('Rebase and merge PR #$pr', {replace: {$pr: prNumber}})
                    : formatMergeBlockReasons(filteredReasons)
                }
                placement="top">
                <Button
                  className="merge-btn"
                  disabled={!canMerge || isMerging}
                  onClick={handleMerge}>
                  {isMerging ? (
                    <>
                      <Icon icon="loading" slot="start" />
                      <T>Merging...</T>
                    </>
                  ) : (
                    <>
                      <Icon icon="git-merge" slot="start" />
                      <T>Rebase and merge</T>
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
              <T>Delete branch after merge</T>
            </label>
            {prsBelow.length > 0 && (
              <div className="merge-close-info">
                <Icon icon="info" size="S" />
                <span>
                  <T replace={{$count: prsBelow.length}}>
                    Will close $count PR(s) below after merge
                  </T>
                </span>
              </div>
            )}
          </div>
        </div>
      </div>

      {!canMerge && filteredReasons.length > 0 && (
        <div className="merge-block-reasons">
          {filteredReasons.map((reason, i) => (
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
