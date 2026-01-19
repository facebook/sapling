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
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {useState} from 'react';
import serverAPI from './ClientToServerAPI';
import {prStacksAtom} from './codeReview/PRStacksAtom';
import {T} from './i18n';
import {useRunOperation} from './operationsState';
import {PullStackOperation} from './operations/PullStackOperation';

import './PRDashboard.css';

export function PRDashboard() {
  const stacks = useAtomValue(prStacksAtom);

  const handleRefresh = () => {
    serverAPI.postMessage({type: 'fetchDiffSummaries'});
  };

  return (
    <div className="pr-dashboard">
      <div className="pr-dashboard-header">
        <span className="pr-dashboard-title">
          <T>PR Stacks</T>
        </span>
        <Tooltip title="Refresh PR list">
          <Button icon onClick={handleRefresh}>
            <Icon icon="refresh" />
          </Button>
        </Tooltip>
      </div>
      <div className="pr-dashboard-content">
        {stacks.length === 0 ? (
          <div className="pr-dashboard-empty">
            <Icon icon="git-pull-request" />
            <span>
              <T>No pull requests found</T>
            </span>
          </div>
        ) : (
          stacks.map(stack => <StackCard key={stack.id} stack={stack} />)
        )}
      </div>
    </div>
  );
}

function StackCard({stack}: {stack: PRStack}) {
  const [isExpanded, setIsExpanded] = useState(true);
  const runOperation = useRunOperation();

  const handlePullStack = () => {
    runOperation(new PullStackOperation(stack.topPrNumber, /* goto */ true));
  };

  const toggleExpanded = () => {
    setIsExpanded(prev => !prev);
  };

  const headerTitle = stack.isStack
    ? `Stack (${stack.prs.length} PRs)`
    : `PR #${stack.topPrNumber}`;

  return (
    <div className="stack-card">
      <div className="stack-card-header">
        <Button className="stack-card-expand-button" onClick={toggleExpanded}>
          <Icon icon={isExpanded ? 'chevron-down' : 'chevron-right'} />
        </Button>
        <span className="stack-card-title" onClick={toggleExpanded}>
          {headerTitle}
        </span>
        <Tooltip title={`Pull ${stack.isStack ? 'stack' : 'PR'} and checkout`}>
          <Button className="stack-card-pull-button" onClick={handlePullStack}>
            <Icon icon="cloud-download" />
            <T>Pull</T>
          </Button>
        </Tooltip>
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

function PRRow({pr}: {pr: DiffSummary}) {
  const stateIcon = getPRStateIcon(pr.state);
  const stateClass = getPRStateClass(pr.state);

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
