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
import {useRunOperation} from './operationsState';
import {PullStackOperation} from './operations/PullStackOperation';

import './PRDashboard.css';

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

  const customLabel = stackLabels[stack.id];
  // Check if this stack is from an external author (someone other than the current user)
  const isExternal = currentUser != null && stack.mainAuthor != null && stack.mainAuthor !== currentUser;

  const handlePullStack = () => {
    runOperation(new PullStackOperation(stack.topPrNumber, /* goto */ true));
  };

  const toggleExpanded = () => {
    setIsExpanded(prev => !prev);
  };

  const handleLabelChange = useCallback(
    (newLabel: string) => {
      setStackLabels(prev => {
        if (newLabel.trim() === '') {
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
  ].filter(Boolean).join(' ');

  return (
    <div className={stackCardClass}>
      <div className="stack-card-header">
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

  const handleViewChanges = () => {
    if (headHash) {
      showComparison({type: ComparisonType.Committed, hash: headHash});
    }
  };

  return (
    <div className="pr-row">
      <span className={`pr-row-status ${stateClass}`}>{stateIcon}</span>
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
