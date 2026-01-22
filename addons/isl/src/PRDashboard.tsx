/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PRStack} from './codeReview/PRStacksAtom';
import type {DiffSummary} from './types';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {TextField} from 'isl-components/TextField';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom, useAtomValue} from 'jotai';
import {useState, useCallback} from 'react';
import {ComparisonType} from 'shared/Comparison';
import serverAPI from './ClientToServerAPI';
import {showComparison} from './ComparisonView/atoms';
import {currentGitHubUser} from './codeReview/CodeReviewInfo';
import {
  prStacksAtom,
  stackLabelsAtom,
  hiddenStacksAtom,
} from './codeReview/PRStacksAtom';
import {T} from './i18n';
import {inlineProgressByHash, useRunOperation} from './operationsState';
import {PullOperation} from './operations/PullOperation';
import {GotoOperation} from './operations/GotoOperation';
import {PullStackOperation} from './operations/PullStackOperation';
import {dagWithPreviews} from './previews';
import {succeedableRevset} from './types';

import './PRDashboard.css';

function MainBranchSection() {
  const runOperation = useRunOperation();
  const dag = useAtomValue(dagWithPreviews);

  // Find main/master bookmark in the dag
  const mainCommit = dag.resolve('main') ?? dag.resolve('master');
  const remoteName = mainCommit?.remoteBookmarks.find(b =>
    b === 'origin/main' || b === 'origin/master' || b === 'remote/main' || b === 'remote/master'
  ) ?? 'main';

  // Check if we're currently on main
  const currentCommit = dag.resolve('.');
  const isOnMain = currentCommit?.hash === mainCommit?.hash;

  // Get inline progress for feedback
  const inlineProgress = useAtomValue(inlineProgressByHash(mainCommit?.hash ?? ''));

  // Calculate sync status (how far behind remote main we are)
  // This is a simplified version - we check if local main differs from remote main
  const remoteMain = dag.resolve('origin/main') ?? dag.resolve('origin/master');
  const isBehind = remoteMain && mainCommit && remoteMain.hash !== mainCommit.hash;

  const handleGoToMain = useCallback(async () => {
    if (isOnMain && !isBehind) {
      return;
    }

    // Pull first to get latest, then goto
    await runOperation(new PullOperation());
    runOperation(new GotoOperation(succeedableRevset(remoteName)));
  }, [isOnMain, isBehind, runOperation, remoteName]);

  const syncStatusText = isBehind
    ? 'Updates available'
    : isOnMain
      ? 'You are here'
      : 'Up to date';

  return (
    <div className="main-branch-section">
      <div className="main-branch-info">
        <Icon icon="git-branch" />
        <span className="main-branch-name">{remoteName.replace('origin/', '')}</span>
        <span className="main-branch-status">{syncStatusText}</span>
      </div>
      <Tooltip title={isOnMain && !isBehind ? 'Already on main' : 'Pull latest and checkout main'}>
        <Button
          className="main-branch-goto-button"
          onClick={handleGoToMain}
          disabled={isOnMain && !isBehind || inlineProgress != null}
        >
          {inlineProgress ? (
            <Icon icon="loading" />
          ) : (
            <Icon icon="arrow-down" />
          )}
          <T>Go to main</T>
        </Button>
      </Tooltip>
    </div>
  );
}

export function PRDashboard() {
  const stacks = useAtomValue(prStacksAtom);
  const [hiddenStacks, setHiddenStacks] = useAtom(hiddenStacksAtom);
  const [showHidden, setShowHidden] = useState(false);

  const handleRefresh = () => {
    serverAPI.postMessage({type: 'fetchDiffSummaries'});
  };

  const visibleStacks = showHidden
    ? stacks
    : stacks.filter(stack => !hiddenStacks.includes(stack.id));

  const hiddenCount = stacks.filter(stack =>
    hiddenStacks.includes(stack.id),
  ).length;

  return (
    <div className="pr-dashboard">
      <div className="pr-dashboard-header">
        <span className="pr-dashboard-title">
          <T>PR Stacks</T>
        </span>
        <div className="pr-dashboard-header-buttons">
          {hiddenCount > 0 && (
            <Tooltip
              title={showHidden ? 'Hide hidden stacks' : 'Show hidden stacks'}>
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
      <MainBranchSection />
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

  const customLabel = stackLabels[stack.id];
  // Check if this stack is from an external author (someone other than the current user)
  const isExternal = currentUser != null && stack.mainAuthor != null && stack.mainAuthor !== currentUser;

  // Get the top PR's head hash for checkout
  const topHeadHash = stack.prs[0]?.type === 'github' ? stack.prs[0].head : undefined;
  const isCurrentStack = topHeadHash ? dag.resolve('.')?.hash === topHeadHash : false;
  const inlineProgress = useAtomValue(inlineProgressByHash(topHeadHash ?? ''));

  const handlePullStack = () => {
    runOperation(new PullStackOperation(stack.topPrNumber, /* goto */ true));
  };

  const handleStackCheckout = useCallback((e: React.MouseEvent) => {
    // Don't interfere with child element clicks
    if ((e.target as HTMLElement).closest('button, input, .stack-card-title')) {
      return;
    }
    if (!topHeadHash || isCurrentStack) {
      return;
    }
    runOperation(new GotoOperation(succeedableRevset(topHeadHash)));
  }, [topHeadHash, isCurrentStack, runOperation]);

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
  ].filter(Boolean).join(' ');

  const headerClass = [
    'stack-card-header',
    topHeadHash && !isCurrentStack ? 'stack-card-header-clickable' : '',
  ].filter(Boolean).join(' ');

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
          <Tooltip title={isHidden ? 'Show stack' : 'Hide stack'}>
            <Button icon onClick={onToggleHidden}>
              <Icon icon={isHidden ? 'eye' : 'eye-closed'} />
            </Button>
          </Tooltip>
          <Tooltip title={`Pull ${stack.isStack ? 'stack' : 'PR'} and checkout`}>
            <Button className="stack-card-pull-button" onClick={handlePullStack}>
              <Icon icon="cloud-download" />
              <T>Pull</T>
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
  const stateIcon = getPRStateIcon(pr.state);
  const stateClass = getPRStateClass(pr.state);
  const author = pr.type === 'github' ? pr.author : undefined;
  const headHash = pr.type === 'github' ? pr.head : undefined;

  const runOperation = useRunOperation();
  const dag = useAtomValue(dagWithPreviews);
  const isCurrentCommit = headHash ? dag.resolve('.')?.hash === headHash : false;
  const inlineProgress = useAtomValue(inlineProgressByHash(headHash ?? ''));

  const handleCheckout = useCallback(() => {
    if (!headHash || isCurrentCommit) {
      return;
    }
    runOperation(new GotoOperation(succeedableRevset(headHash)));
  }, [headHash, isCurrentCommit, runOperation]);

  const handleViewChanges = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (headHash) {
      showComparison({type: ComparisonType.Committed, hash: headHash});
    }
  };

  const prRowClass = [
    'pr-row',
    headHash && !isCurrentCommit ? 'pr-row-clickable' : '',
    isCurrentCommit ? 'pr-row-current' : '',
    inlineProgress ? 'pr-row-loading' : '',
  ].filter(Boolean).join(' ');

  return (
    <div className={prRowClass} onClick={handleCheckout}>
      {inlineProgress ? (
        <Icon icon="loading" className="pr-row-status" />
      ) : (
        <span className={`pr-row-status ${stateClass}`}>{stateIcon}</span>
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
      {author && <span className="pr-row-author">@{author}</span>}
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

function getPRStateIcon(state: DiffSummary['state']): string {
  switch (state) {
    case 'MERGED':
      return '✓';
    case 'CLOSED':
      return '✕';
    case 'DRAFT':
      return '○';
    case 'MERGE_QUEUED':
      return '◐';
    case 'OPEN':
    default:
      return '●';
  }
}

function getPRStateClass(state: DiffSummary['state']): string {
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
      return 'pr-state-open';
  }
}
