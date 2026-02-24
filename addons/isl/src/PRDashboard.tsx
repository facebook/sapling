/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PRStack} from './codeReview/PRStacksAtom';
import type {DiffSummary, TimeRangeDays} from './types';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {TextField} from 'isl-components/TextField';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom, useAtomValue} from 'jotai';
import {useCallback, useEffect, useRef, useState} from 'react';
import {ComparisonType} from 'shared/Comparison';
import serverAPI from './ClientToServerAPI';
import {
  allDiffSummaries,
  currentGitHubUser,
  triggerFullDiffSummariesRefresh,
} from './codeReview/CodeReviewInfo';
import {
  hiddenStacksAtom,
  hideBotStacksAtom,
  hideMergedStacksAtom,
  isBotAuthor,
  prStacksAtom,
  showOnlyMyStacksAtom,
  stackLabelsAtom,
} from './codeReview/PRStacksAtom';
import {scrollToCommit} from './CommitTreeList';
import {showComparison} from './ComparisonView/atoms';
import {T, t} from './i18n';
import {writeAtom} from './jotaiUtils';
import {ClosePROperation} from './operations/ClosePROperation';
import {GotoOperation} from './operations/GotoOperation';
import {PullOperation} from './operations/PullOperation';
import {PullRevOperation} from './operations/PullRevOperation';
import {WorktreeAddOperation} from './operations/WorktreeAddOperation';
import {inlineProgressByHash, useRunOperation} from './operationsState';
import platform from './platform';
import {dagWithPreviews} from './previews';
import {enterReviewMode} from './reviewMode';
import {selectedCommits} from './selection';
import {repositoryInfo, selectedTimeRangeAtom, setTimeRange} from './serverAPIState';
import {showToast} from './toast';
import {exactRevset, succeedableRevset} from './types';
import {showWorktreeOpenInIDEModal} from './WorktreeIDEModal';
import {worktreesAtom, worktreesForCommit} from './worktrees';

import './PRDashboard.css';

/**
 * Skeleton loading state for the PR Dashboard.
 * Shows animated placeholders while data is being fetched.
 */
function PRDashboardSkeleton() {
  // Varying PR counts to look natural: stack of 3, stack of 2, single, stack of 2, single, stack of 3, single, single
  const skeletonConfigs = [3, 2, 1, 2, 1, 3, 1, 1];
  return (
    <div className="pr-dashboard pr-dashboard-skeleton">
      <div className="pr-dashboard-sticky-header">
        <div className="pr-dashboard-header">
          <span className="pr-dashboard-title">
            <T>PR Stacks</T>
          </span>
        </div>
      </div>
      <div className="pr-dashboard-content">
        {skeletonConfigs.map((prCount, i) => (
          <StackCardSkeleton key={i} prCount={prCount} />
        ))}
      </div>
    </div>
  );
}

/**
 * Skeleton for a single stack card.
 */
function StackCardSkeleton({prCount = 2}: {prCount?: number}) {
  return (
    <div className="stack-card stack-card-skeleton">
      <div className="stack-card-header">
        <div className="skeleton-box skeleton-icon" />
        <div className="skeleton-box skeleton-title" />
        <div className="skeleton-box skeleton-avatar" />
      </div>
      <div className="stack-card-prs">
        {Array.from({length: prCount}, (_, i) => (
          <PRRowSkeleton key={i} />
        ))}
      </div>
    </div>
  );
}

/**
 * Skeleton for a single PR row.
 */
function PRRowSkeleton() {
  return (
    <div className="pr-row pr-row-skeleton">
      <div className="skeleton-box skeleton-dot" />
      <div className="skeleton-box skeleton-pr-number" />
      <div className="skeleton-box skeleton-pr-title" />
    </div>
  );
}

/**
 * Scroll the PR column to show a PR row at the top.
 * Uses native scrollIntoView with CSS scroll-margin-top for padding.
 */
function scrollToPR(hash: string): void {
  const element = document.getElementById(`pr-${hash}`);
  element?.scrollIntoView({behavior: 'smooth', block: 'start'});
}

/**
 * Hook to scroll the PR column when a commit is selected in the middle column.
 */
function useScrollToPROnSelection() {
  const selected = useAtomValue(selectedCommits);

  useEffect(() => {
    if (selected.size !== 1) {
      return;
    }
    const hash = Array.from(selected)[0];
    scrollToPR(hash);
  }, [selected]);
}

function MainBranchSection({}: {isScrolled?: boolean}) {
  const runOperation = useRunOperation();
  const dag = useAtomValue(dagWithPreviews);
  const repoInfo = useAtomValue(repositoryInfo);
  const worktrees = useAtomValue(worktreesAtom);

  // Find main/master bookmark in the dag
  const mainCommit = dag.resolve('main') ?? dag.resolve('master');
  const remoteName =
    mainCommit?.remoteBookmarks.find(
      b =>
        b === 'origin/main' ||
        b === 'origin/master' ||
        b === 'remote/main' ||
        b === 'remote/master',
    ) ?? 'main';

  // Check if we're currently on main
  const currentCommit = dag.resolve('.');
  const isOnMain = currentCommit?.hash === mainCommit?.hash;

  // Check if we're in a secondary worktree (not the main worktree)
  const mainWorktree = worktrees.find(wt => wt.isMain);
  const isInMainWorktree = mainWorktree?.path === repoInfo?.repoRoot;
  const isInSecondaryWorktree = mainWorktree != null && !isInMainWorktree;

  // Get inline progress for feedback
  const inlineProgress = useAtomValue(inlineProgressByHash(mainCommit?.hash ?? ''));

  // Calculate sync status (how far behind remote main we are)
  // This is a simplified version - we check if local main differs from remote main
  const remoteMain = dag.resolve('origin/main') ?? dag.resolve('origin/master');
  const isBehind = remoteMain && mainCommit && remoteMain.hash !== mainCommit.hash;

  const handleGoToMain = useCallback(async () => {
    if (isInSecondaryWorktree && mainWorktree) {
      // Switch ISL to the main worktree first
      serverAPI.postMessage({type: 'changeCwd', cwd: mainWorktree.path});
      serverAPI.cwdChanged();
      if (platform.platformName !== 'vscode') {
        showWorktreeOpenInIDEModal(mainWorktree.path, mainWorktree.name);
      }
      // Pull will happen after ISL reconnects to the main worktree
      return;
    }

    if (isOnMain && !isBehind) {
      return;
    }

    // Pull first to get latest, then goto
    await runOperation(new PullOperation());
    runOperation(new GotoOperation(succeedableRevset(remoteName)));
  }, [isOnMain, isBehind, isInSecondaryWorktree, mainWorktree, runOperation, remoteName]);

  const syncStatusText = isInSecondaryWorktree
    ? 'In worktree'
    : isBehind
      ? 'Updates available'
      : isOnMain
        ? 'You are here'
        : 'Up to date';

  const statusClass = isBehind || isInSecondaryWorktree
    ? 'main-branch-status main-branch-status-behind'
    : 'main-branch-status';

  const tooltipText = isInSecondaryWorktree
    ? 'Switch back to main worktree'
    : isOnMain && !isBehind
      ? 'Already on main'
      : 'Pull latest and checkout main';

  const isDisabled = !isInSecondaryWorktree && isOnMain && !isBehind;

  return (
    <div className="main-branch-section">
      <div className="main-branch-info">
        <Icon icon="git-branch" />
        <span className="main-branch-name">{remoteName.replace('origin/', '')}</span>
        <span className={statusClass}>{syncStatusText}</span>
      </div>
      <Tooltip title={tooltipText}>
        <Button
          className="main-branch-goto-button"
          onClick={handleGoToMain}
          disabled={isDisabled || inlineProgress != null}>
          {inlineProgress ? <Icon icon="loading" /> : <Icon icon="arrow-down" />}
          <T>Go to main</T>
        </Button>
      </Tooltip>
    </div>
  );
}

const TIME_RANGE_OPTIONS: Array<{value: TimeRangeDays; label: string}> = [
  {value: 7, label: '7 days'},
  {value: 14, label: '14 days'},
  {value: 30, label: '30 days'},
  {value: undefined, label: 'All time'},
];

function TimeRangeDropdown() {
  const selectedRange = useAtomValue(selectedTimeRangeAtom);

  const handleChange = (event: React.ChangeEvent<HTMLSelectElement>) => {
    const value = event.target.value;
    const days = value === 'undefined' ? undefined : (parseInt(value, 10) as TimeRangeDays);
    setTimeRange(days);
  };

  return (
    <Tooltip title="Filter PRs and commits by date range">
      <select
        className="time-range-dropdown"
        value={selectedRange === undefined ? 'undefined' : String(selectedRange)}
        onChange={handleChange}>
        {TIME_RANGE_OPTIONS.map(opt => (
          <option
            key={opt.value ?? 'undefined'}
            value={opt.value === undefined ? 'undefined' : String(opt.value)}>
            {opt.label}
          </option>
        ))}
      </select>
    </Tooltip>
  );
}

export function PRDashboard() {
  const diffSummariesResult = useAtomValue(allDiffSummaries);
  const stacks = useAtomValue(prStacksAtom);
  const [hiddenStacks, setHiddenStacks] = useAtom(hiddenStacksAtom);
  const [hideMerged, setHideMerged] = useAtom(hideMergedStacksAtom);
  const [showOnlyMine, setShowOnlyMine] = useAtom(showOnlyMyStacksAtom);
  const [hideBots, setHideBots] = useAtom(hideBotStacksAtom);
  const [showHidden, setShowHidden] = useState(false);
  const [isScrolled, setIsScrolled] = useState(false);
  const currentUser = useAtomValue(currentGitHubUser);
  const dashboardRef = useRef<HTMLDivElement>(null);

  // Scroll to PR row when a commit is selected in the middle column
  useScrollToPROnSelection();

  // Detect scroll for blur effect on sticky headers
  useEffect(() => {
    // Find the scrollable parent (the drawer content wrapper)
    let container: Element | null = dashboardRef.current?.parentElement ?? null;
    while (container && getComputedStyle(container).overflowY !== 'auto') {
      container = container.parentElement;
    }
    if (!container) return;

    const handleScroll = () => {
      setIsScrolled(container!.scrollTop > 10);
    };

    handleScroll(); // Check initial
    container.addEventListener('scroll', handleScroll, {passive: true});
    return () => container!.removeEventListener('scroll', handleScroll);
  }, []);

  // Show skeleton while loading (value is null) - must be after all hooks
  const isLoading = diffSummariesResult.value === null && !diffSummariesResult.error;
  if (isLoading) {
    return <PRDashboardSkeleton />;
  }

  const handleRefresh = () => {
    // Use full refresh to replace (not merge) so closed PRs disappear
    triggerFullDiffSummariesRefresh();
  };

  // Filter stacks: always hide closed, hide manually hidden, optionally merged, bots, and optionally non-mine stacks
  const visibleStacks = showHidden
    ? stacks.filter(stack => !stack.isClosed) // Still hide closed even when showing hidden
    : stacks.filter(stack => {
        if (stack.isClosed) return false; // Always hide closed (abandoned) PRs
        if (hiddenStacks.includes(stack.id)) return false;
        if (hideMerged && stack.isMerged) return false;
        if (hideBots && isBotAuthor(stack.mainAuthor)) return false;
        if (showOnlyMine && currentUser && stack.mainAuthor !== currentUser) return false;
        return true;
      });

  const hiddenCount = stacks.filter(stack => hiddenStacks.includes(stack.id)).length;

  const mergedCount = stacks.filter(stack => stack.isMerged).length;

  const botCount = stacks.filter(stack => isBotAuthor(stack.mainAuthor)).length;

  const otherAuthorsCount = currentUser
    ? stacks.filter(stack => stack.mainAuthor && stack.mainAuthor !== currentUser).length
    : 0;

  return (
    <div className="pr-dashboard" ref={dashboardRef}>
      {/* Unified sticky header */}
      <div className="pr-dashboard-sticky-header">
        <div className="pr-dashboard-header">
          <span className="pr-dashboard-title">
            <T>PR Stacks</T> <span style={{fontSize: '10px', opacity: 0.5}}>(v4.2.1)</span>
          </span>
          <div className="pr-dashboard-header-buttons">
            <TimeRangeDropdown />
            {currentUser && (
              <Tooltip
                title={
                  showOnlyMine
                    ? `Show all authors (${otherAuthorsCount} hidden)`
                    : 'Show only my stacks'
                }>
                <Button
                  icon
                  onClick={() => setShowOnlyMine(prev => !prev)}
                  className={showOnlyMine ? 'author-filter-active' : 'author-filter-inactive'}>
                  <Icon icon="account" />
                  {showOnlyMine && otherAuthorsCount > 0 && (
                    <span className="hidden-count">{otherAuthorsCount}</span>
                  )}
                </Button>
              </Tooltip>
            )}
            <Tooltip
              title={
                hideBots ? `Show ${botCount} bot PRs` : 'Hide bot PRs (renovate, dependabot, etc)'
              }>
              <Button
                icon
                onClick={() => setHideBots(prev => !prev)}
                className={hideBots ? 'bot-filter-active' : 'bot-filter-inactive'}>
                <Icon icon="hubot" />
                {hideBots && botCount > 0 && <span className="hidden-count">{botCount}</span>}
              </Button>
            </Tooltip>
            <Tooltip title={hideMerged ? `Show ${mergedCount} merged` : 'Hide merged stacks'}>
              <Button
                icon
                onClick={() => setHideMerged(prev => !prev)}
                className={hideMerged ? 'merged-toggle-hidden' : 'merged-toggle-visible'}>
                <Icon icon="check" />
                {mergedCount > 0 && <span className="hidden-count">{mergedCount}</span>}
              </Button>
            </Tooltip>
            {hiddenCount > 0 && (
              <Tooltip title={showHidden ? 'Hide hidden stacks' : 'Show hidden stacks'}>
                <Button icon onClick={() => setShowHidden(prev => !prev)}>
                  <Icon icon={showHidden ? 'eye' : 'eye-closed'} />
                  <span className="hidden-count">{hiddenCount}</span>
                </Button>
              </Tooltip>
            )}
            <Tooltip title="Refresh PR list">
              <Button icon onClick={handleRefresh}>
                <Icon icon="refresh" />
              </Button>
            </Tooltip>
          </div>
        </div>
        <MainBranchSection isScrolled={isScrolled} />
      </div>
      <div className="pr-dashboard-content">
        {visibleStacks.length === 0 ? (
          <div className="pr-dashboard-empty">
            <Icon icon="git-pull-request" />
            <span>
              <T>No pull requests found</T>
            </span>
          </div>
        ) : (
          visibleStacks.map(stack => (
            <StackCard
              key={stack.id}
              stack={stack}
              isHidden={hiddenStacks.includes(stack.id)}
              onToggleHidden={() => {
                setHiddenStacks(prev =>
                  prev.includes(stack.id)
                    ? prev.filter(id => id !== stack.id)
                    : [...prev, stack.id],
                );
              }}
            />
          ))
        )}
      </div>
    </div>
  );
}

function StackCard({
  stack,
  isHidden,
  onToggleHidden,
}: {
  stack: PRStack;
  isHidden: boolean;
  onToggleHidden: () => void;
}) {
  const [isExpanded, setIsExpanded] = useState(true);
  const [isEditingLabel, setIsEditingLabel] = useState(false);
  const [stackLabels, setStackLabels] = useAtom(stackLabelsAtom);
  const runOperation = useRunOperation();
  const currentUser = useAtomValue(currentGitHubUser);
  const dag = useAtomValue(dagWithPreviews);
  const repoInfo = useAtomValue(repositoryInfo);
  const currentRepoRoot = repoInfo?.repoRoot;

  const customLabel = stackLabels[stack.id];
  // Check if this stack is from an external author (someone other than the current user)
  const isExternal =
    currentUser != null && stack.mainAuthor != null && stack.mainAuthor !== currentUser;

  // Get the top PR's head hash for checkout
  const topHeadHash = stack.prs[0]?.type === 'github' ? stack.prs[0].head : undefined;
  const isCurrentStack = topHeadHash ? dag.resolve('.')?.hash === topHeadHash : false;
  const inlineProgress = useAtomValue(inlineProgressByHash(topHeadHash ?? ''));

  // Check if a worktree already exists for this commit
  const existingWorktrees = useAtomValue(worktreesForCommit(topHeadHash ?? ''));
  const existingWorktree = existingWorktrees.length > 0 ? existingWorktrees[0] : undefined;
  // Don't show worktree button if we're already in that worktree
  const isInExistingWorktree =
    existingWorktree != null && existingWorktree.path === currentRepoRoot;

  // Detect "stale" stacks: the true top PR (from stackInfo) was merged via GitHub
  // but the lower PRs are still open. All visible PRs in this stack are stale.
  const stalePRs = stack.hasStaleAbove
    ? stack.prs.filter(pr => pr.state !== 'MERGED' && pr.state !== 'CLOSED')
    : [];
  const hasStaleStack = stack.hasStaleAbove && stalePRs.length > 0;

  const [isClosingStale, setIsClosingStale] = useState(false);

  const handleCloseStalePRs = useCallback(async () => {
    if (stalePRs.length === 0 || isClosingStale) return;

    setIsClosingStale(true);
    const mergedPrNumber = stack.mergedAbovePrNumber ?? stack.topPrNumber;
    showToast(t('Closing $count stale PRs...', {replace: {$count: String(stalePRs.length)}}), {
      durationMs: 3000,
    });

    for (const pr of stalePRs) {
      try {
        const closeOp = new ClosePROperation(
          Number(pr.number),
          `Closed: changes already merged via PR #${mergedPrNumber}`,
        );
        await runOperation(closeOp);
      } catch (err) {
        // Continue closing others even if one fails
      }
    }

    showToast(t('Closed $count stale PRs', {replace: {$count: String(stalePRs.length)}}), {
      durationMs: 3000,
    });
    setIsClosingStale(false);

    // Refresh the PR list after a short delay to let GitHub propagate the changes
    // Use full refresh to replace (not merge) so closed PRs disappear
    setTimeout(() => {
      triggerFullDiffSummariesRefresh();
    }, 1500);
  }, [stalePRs, isClosingStale, stack.mergedAbovePrNumber, stack.topPrNumber, runOperation]);

  const handleStackCheckout = useCallback(
    (e: React.MouseEvent) => {
      // Don't interfere with child element clicks
      if ((e.target as HTMLElement).closest('button, input, .stack-card-title')) {
        return;
      }
      if (!topHeadHash || isCurrentStack) {
        return;
      }
      // If commit is checked out in another worktree, switch to it instead of goto
      if (existingWorktree && !isInExistingWorktree) {
        serverAPI.postMessage({type: 'changeCwd', cwd: existingWorktree.path});
        serverAPI.cwdChanged();
        if (platform.platformName !== 'vscode') {
          showWorktreeOpenInIDEModal(existingWorktree.path, existingWorktree.name);
        }
        return;
      }
      runOperation(new GotoOperation(succeedableRevset(topHeadHash)));
    },
    [topHeadHash, isCurrentStack, existingWorktree, isInExistingWorktree, runOperation],
  );

  const toggleExpanded = () => {
    setIsExpanded(prev => !prev);
  };

  const handleLabelChange = useCallback(
    (newLabel: string) => {
      setStackLabels(prev => {
        if (newLabel.trim() === '') {
          // eslint-disable-next-line @typescript-eslint/no-unused-vars
          const {[stack.id]: _, ...rest} = prev;
          return rest;
        }
        return {...prev, [stack.id]: newLabel.trim()};
      });
      setIsEditingLabel(false);
    },
    [stack.id, setStackLabels],
  );

  const defaultTitle = stack.isStack
    ? `Stack (${stack.prs.length} PRs)`
    : `PR #${stack.topPrNumber}`;

  const headerTitle = customLabel || defaultTitle;

  const stackCardClass = [
    'stack-card',
    isHidden ? 'stack-card-hidden' : '',
    isExternal ? 'stack-card-external' : '',
    isCurrentStack ? 'stack-card-current' : '',
    inlineProgress ? 'stack-card-loading' : '',
    stack.isMerged ? 'stack-card-merged' : '',
  ]
    .filter(Boolean)
    .join(' ');

  const headerClass = [
    'stack-card-header',
    topHeadHash && !isCurrentStack ? 'stack-card-header-clickable' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div className={stackCardClass}>
      <div className={headerClass} onClick={handleStackCheckout}>
        <Button className="stack-card-expand-button" onClick={toggleExpanded}>
          <Icon icon={isExpanded ? 'chevron-down' : 'chevron-right'} />
        </Button>

        {isEditingLabel ? (
          <LabelEditor
            initialValue={customLabel || ''}
            onSave={handleLabelChange}
            onCancel={() => setIsEditingLabel(false)}
          />
        ) : (
          <span
            className="stack-card-title"
            onClick={toggleExpanded}
            onDoubleClick={() => setIsEditingLabel(true)}
            title="Double-click to edit label">
            {headerTitle}
          </span>
        )}

        {/* Merge status badge */}
        {stack.isMerged ? (
          <span className="stack-merge-badge stack-merge-badge-merged">
            <Icon icon="check" /> Merged
          </span>
        ) : stack.mergedCount + stack.closedCount > 0 ? (
          <span className="stack-merge-badge stack-merge-badge-partial">
            {stack.mergedCount + stack.closedCount}/{stack.prs.length}
          </span>
        ) : null}

        {stack.mainAuthor && (
          <Tooltip title={stack.mainAuthor}>
            <span className="stack-card-author">
              {stack.mainAuthorAvatarUrl ? (
                <img
                  src={stack.mainAuthorAvatarUrl}
                  alt={stack.mainAuthor}
                  className="stack-card-avatar"
                />
              ) : (
                <Icon icon="account" />
              )}
            </span>
          </Tooltip>
        )}

        <div className="stack-card-actions">
          {/* Show "Switch to Worktree" for ANY stack checked out in another worktree */}
          {existingWorktree &&
            topHeadHash &&
            !isInExistingWorktree && (
              <Tooltip title="Switch ISL to the existing worktree for this stack">
                <Button
                  className="stack-card-worktree-button"
                  onClick={(e: React.MouseEvent) => {
                    e.stopPropagation();
                    serverAPI.postMessage({
                      type: 'changeCwd',
                      cwd: existingWorktree.path,
                    });
                    serverAPI.cwdChanged();
                    if (platform.platformName !== 'vscode') {
                      showWorktreeOpenInIDEModal(existingWorktree.path, existingWorktree.name);
                    }
                  }}>
                  <Icon icon="go-to-file" />
                  <T>Switch to Worktree</T>
                </Button>
              </Tooltip>
            )}
          {/* Show "Open in Worktree" only for external stacks without an existing worktree */}
          {isExternal &&
            topHeadHash &&
            !isInExistingWorktree &&
            !existingWorktree && (
              <Tooltip title="Open this stack in a new worktree">
                <Button
                  className="stack-card-worktree-button"
                  onClick={async (e: React.MouseEvent) => {
                    e.stopPropagation();
                    // Pull the commit first so it exists locally, then create worktree
                    await runOperation(new PullRevOperation(exactRevset(topHeadHash)));
                    runOperation(new WorktreeAddOperation(topHeadHash));
                  }}>
                  <Icon icon="folder-opened" />
                  <T>Open in Worktree</T>
                </Button>
              </Tooltip>
            )}
          {hasStaleStack && (
            <Tooltip
              title={`Close ${stalePRs.length} stale PR${stalePRs.length > 1 ? 's' : ''} — these PRs are still open but their changes were already merged via PR #${stack.mergedAbovePrNumber ?? '?'} on GitHub. This happens when merging directly on GitHub instead of through ISL.`}>
              <Button
                className="stack-card-close-stale-button"
                onClick={handleCloseStalePRs}
                disabled={isClosingStale}>
                {isClosingStale ? <Icon icon="loading" /> : <Icon icon="trash" />}
                <span>Close {stalePRs.length} stale</span>
              </Button>
            </Tooltip>
          )}
          <Tooltip title={isHidden ? 'Show stack' : 'Hide stack'}>
            <Button icon onClick={onToggleHidden}>
              <Icon icon={isHidden ? 'eye' : 'eye-closed'} />
            </Button>
          </Tooltip>
        </div>
      </div>
      {isExpanded && (
        <div className="stack-card-prs">
          {stack.prs.map(pr => (
            <PRRow key={pr.number} pr={pr} />
          ))}
        </div>
      )}
    </div>
  );
}

function LabelEditor({
  initialValue,
  onSave,
  onCancel,
}: {
  initialValue: string;
  onSave: (value: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initialValue);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      onSave(value);
    } else if (e.key === 'Escape') {
      onCancel();
    }
  };

  return (
    <TextField
      autoFocus
      value={value}
      onChange={e => setValue((e.target as HTMLInputElement).value)}
      onKeyDown={handleKeyDown}
      onBlur={() => onSave(value)}
      placeholder="Enter label..."
      className="stack-card-label-input"
    />
  );
}

function PRRow({pr}: {pr: DiffSummary}) {
  const reviewDecision = pr.type === 'github' ? pr.reviewDecision : undefined;
  const stateIcon = getPRStateIcon(pr.state, reviewDecision);
  const stateClass = getPRStateClass(pr.state, reviewDecision);
  const headHash = pr.type === 'github' && pr.head !== '' ? pr.head : undefined;
  const isMerged = pr.state === 'MERGED';
  const isClosed = pr.state === 'CLOSED';

  const runOperation = useRunOperation();
  const dag = useAtomValue(dagWithPreviews);
  const repoInfo = useAtomValue(repositoryInfo);
  const isCurrentCommit = headHash ? dag.resolve('.')?.hash === headHash : false;
  const inlineProgress = useAtomValue(inlineProgressByHash(headHash ?? ''));

  // Check if this commit is already checked out in a worktree
  const existingWorktrees = useAtomValue(worktreesForCommit(headHash ?? ''));
  const existingWorktree = existingWorktrees.length > 0 ? existingWorktrees[0] : undefined;
  const isInExistingWorktree =
    existingWorktree != null && existingWorktree.path === repoInfo?.repoRoot;

  const handleCheckout = useCallback(() => {
    if (!headHash) {
      return;
    }
    // Select the commit (this also triggers scroll via useScrollToSelectedCommit hook)
    writeAtom(selectedCommits, new Set([headHash]));
    // Also explicitly scroll in case the hook doesn't fire (e.g., same selection)
    scrollToCommit(headHash);
    if (!isCurrentCommit) {
      // If commit is checked out in another worktree, switch to it instead of goto
      if (existingWorktree && !isInExistingWorktree) {
        serverAPI.postMessage({type: 'changeCwd', cwd: existingWorktree.path});
        serverAPI.cwdChanged();
        if (platform.platformName !== 'vscode') {
          showWorktreeOpenInIDEModal(existingWorktree.path, existingWorktree.name);
        }
        return;
      }
      runOperation(new GotoOperation(succeedableRevset(headHash)));
    }
    // Scroll again after operation completes and React re-renders
    setTimeout(() => scrollToCommit(headHash), 500);
  }, [headHash, isCurrentCommit, existingWorktree, isInExistingWorktree, runOperation]);

  const handleViewChanges = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (headHash) {
      showComparison({type: ComparisonType.Committed, hash: headHash});
    }
  };

  const handleReview = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      if (headHash) {
        enterReviewMode(pr.number, headHash);
      }
    },
    [pr.number, headHash],
  );

  const prRowClass = [
    'pr-row',
    headHash && !isCurrentCommit ? 'pr-row-clickable' : '',
    isCurrentCommit ? 'pr-row-current' : '',
    inlineProgress ? 'pr-row-loading' : '',
    isMerged ? 'pr-row-merged' : '',
    isClosed ? 'pr-row-closed' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div
      className={prRowClass}
      onClick={handleCheckout}
      id={headHash ? `pr-${headHash}` : undefined}>
      {inlineProgress ? (
        <Icon icon="loading" className="pr-row-status" />
      ) : (
        <Tooltip
          component={() => (
            <PRStatusLegend currentState={pr.state} currentReviewDecision={reviewDecision} />
          )}
          interactive
          delayMs={400}
          placement="right">
          <span className={`pr-row-status ${stateClass}`}>{stateIcon}</span>
        </Tooltip>
      )}
      <a
        className="pr-row-number"
        href={pr.url}
        target="_blank"
        rel="noopener noreferrer"
        onClick={e => e.stopPropagation()}>
        #{pr.number}
      </a>
      <span className="pr-row-title" title={pr.title}>
        {pr.title}
      </span>
      {isMerged && (
        <span className="pr-row-merged-badge">
          <Icon icon="check" size="S" />
          <T>Merged</T>
        </span>
      )}
      {isClosed && (
        <span className="pr-row-closed-badge">
          <Icon icon="close" size="S" />
          <T>Closed</T>
        </span>
      )}
      {headHash && (
        <Tooltip title="Enter review mode for this PR">
          <Button icon className="pr-row-review-button" onClick={handleReview}>
            <Icon icon="eye" />
          </Button>
        </Tooltip>
      )}
      {headHash && (
        <Tooltip title="View changes in this commit">
          <Button icon className="pr-row-view-changes" onClick={handleViewChanges}>
            <Icon icon="diff" />
          </Button>
        </Tooltip>
      )}
    </div>
  );
}

function getPRStateIcon(state: DiffSummary['state'], reviewDecision?: string): string {
  switch (state) {
    case 'MERGED':
      return '✓';
    case 'CLOSED':
      return '✕';
    case 'DRAFT':
      return '✎';
    case 'MERGE_QUEUED':
      return '◐';
    case 'OPEN':
    default:
      switch (reviewDecision) {
        case 'APPROVED':
          return '✓';
        case 'CHANGES_REQUESTED':
          return '↻';
        case 'REVIEW_REQUIRED':
          return '◑';
        default:
          return '●';
      }
  }
}

function getPRStateClass(state: DiffSummary['state'], reviewDecision?: string): string {
  switch (state) {
    case 'MERGED':
      return 'pr-state-merged';
    case 'CLOSED':
      return 'pr-state-closed';
    case 'DRAFT':
      return 'pr-state-draft';
    case 'MERGE_QUEUED':
      return 'pr-state-queued';
    case 'OPEN':
    default:
      switch (reviewDecision) {
        case 'APPROVED':
          return 'pr-state-approved';
        case 'CHANGES_REQUESTED':
          return 'pr-state-changes-requested';
        case 'REVIEW_REQUIRED':
          return 'pr-state-review-required';
        default:
          return 'pr-state-open';
      }
  }
}

const PR_STATUS_LEGEND = [
  {icon: '●', label: 'Open', cssClass: 'pr-state-open', state: 'OPEN', review: undefined},
  {icon: '✓', label: 'Approved', cssClass: 'pr-state-approved', state: 'OPEN', review: 'APPROVED'},
  {
    icon: '↻',
    label: 'Changes requested',
    cssClass: 'pr-state-changes-requested',
    state: 'OPEN',
    review: 'CHANGES_REQUESTED',
  },
  {
    icon: '◑',
    label: 'Review required',
    cssClass: 'pr-state-review-required',
    state: 'OPEN',
    review: 'REVIEW_REQUIRED',
  },
  {icon: '✎', label: 'Draft', cssClass: 'pr-state-draft', state: 'DRAFT', review: undefined},
  {
    icon: '◐',
    label: 'In merge queue',
    cssClass: 'pr-state-queued',
    state: 'MERGE_QUEUED',
    review: undefined,
  },
  {icon: '✓', label: 'Merged', cssClass: 'pr-state-merged', state: 'MERGED', review: undefined},
  {icon: '✕', label: 'Closed', cssClass: 'pr-state-closed', state: 'CLOSED', review: undefined},
] as const;

function PRStatusLegend({
  currentState,
  currentReviewDecision,
}: {
  currentState: DiffSummary['state'];
  currentReviewDecision?: string;
}) {
  const currentIcon = getPRStateIcon(currentState, currentReviewDecision);
  const currentClass = getPRStateClass(currentState, currentReviewDecision);
  const currentLabel =
    PR_STATUS_LEGEND.find(
      s =>
        s.state === currentState && (currentState !== 'OPEN' || s.review === currentReviewDecision),
    )?.label ?? 'Open';

  return (
    <div className="pr-status-legend">
      <div className="pr-status-legend-current">
        <span className={`pr-status-legend-icon ${currentClass}`}>{currentIcon}</span>
        <span className="pr-status-legend-current-label">{currentLabel}</span>
      </div>
      <div className="pr-status-legend-divider" />
      <div className="pr-status-legend-grid">
        {PR_STATUS_LEGEND.map(entry => {
          const isCurrent =
            entry.state === currentState &&
            (currentState !== 'OPEN' || entry.review === currentReviewDecision);
          return (
            <div
              key={`${entry.state}-${entry.review}`}
              className={`pr-status-legend-item ${isCurrent ? 'pr-status-legend-item-active' : ''}`}>
              <span className={`pr-status-legend-icon ${entry.cssClass}`}>{entry.icon}</span>
              <span className="pr-status-legend-label">{entry.label}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
