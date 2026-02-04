/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CICheckRun, DiffSignalSummary} from '../types';

import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useState} from 'react';
import {CircleEllipsisIcon} from '../icons/CircleEllipsisIcon';
import './CIStatusBadge.css';

export type CIStatusBadgeProps = {
  signalSummary?: DiffSignalSummary;
  ciChecks?: CICheckRun[];
};

/**
 * Displays CI status with expandable details showing individual check runs.
 * Used in merge controls to show CI status before merging (MRG-01).
 */
export function CIStatusBadge({signalSummary, ciChecks}: CIStatusBadgeProps) {
  const [expanded, setExpanded] = useState(false);

  if (!signalSummary || signalSummary === 'no-signal') {
    return (
      <div className="ci-status-badge ci-status-no-signal">
        <Icon icon="question" />
        <span>No CI</span>
      </div>
    );
  }

  const {icon, label, className} = getStatusDisplay(signalSummary);
  const hasDetails = ciChecks && ciChecks.length > 0;

  return (
    <div className={`ci-status-badge ${className}`}>
      <Tooltip
        title={hasDetails ? 'Click to see check details' : label}
        placement="bottom">
        <button
          className="ci-status-summary"
          onClick={() => hasDetails && setExpanded(!expanded)}
          disabled={!hasDetails}>
          {icon}
          <span>{label}</span>
          {hasDetails && (
            <Icon
              icon={expanded ? 'chevron-up' : 'chevron-down'}
              size="XS"
            />
          )}
        </button>
      </Tooltip>

      {expanded && hasDetails && (
        <div className="ci-status-details">
          {ciChecks.map((check, i) => (
            <CICheckRow key={`${check.name}-${i}`} check={check} />
          ))}
        </div>
      )}
    </div>
  );
}

function CICheckRow({check}: {check: CICheckRun}) {
  const {icon, className} = getCheckStatusDisplay(check);

  const content = (
    <div className={`ci-check-row ${className}`}>
      {icon}
      <span className="ci-check-name">{check.name}</span>
    </div>
  );

  if (check.detailsUrl) {
    return (
      <a
        href={check.detailsUrl}
        target="_blank"
        rel="noopener noreferrer"
        className="ci-check-link">
        {content}
        <Icon icon="link-external" size="XS" />
      </a>
    );
  }

  return content;
}

function getStatusDisplay(signalSummary: DiffSignalSummary): {
  icon: React.ReactNode;
  label: string;
  className: string;
} {
  switch (signalSummary) {
    case 'pass':
      return {
        icon: <Icon icon="check" />,
        label: 'Checks passing',
        className: 'ci-status-pass',
      };
    case 'failed':
      return {
        icon: <Icon icon="error" />,
        label: 'Checks failing',
        className: 'ci-status-failed',
      };
    case 'running':
      return {
        icon: <CircleEllipsisIcon />,
        label: 'Checks running',
        className: 'ci-status-running',
      };
    case 'warning':
      return {
        icon: <Icon icon="warning" />,
        label: 'Some checks failed',
        className: 'ci-status-warning',
      };
    case 'land-cancelled':
      return {
        icon: <Icon icon="warning" />,
        label: 'Land cancelled',
        className: 'ci-status-warning',
      };
    default:
      return {
        icon: <Icon icon="question" />,
        label: 'Unknown status',
        className: 'ci-status-unknown',
      };
  }
}

function getCheckStatusDisplay(check: CICheckRun): {
  icon: React.ReactNode;
  className: string;
} {
  if (check.status !== 'COMPLETED') {
    return {
      icon: <CircleEllipsisIcon />,
      className: 'ci-check-running',
    };
  }

  switch (check.conclusion) {
    case 'SUCCESS':
      return {
        icon: <Icon icon="check" />,
        className: 'ci-check-pass',
      };
    case 'FAILURE':
      return {
        icon: <Icon icon="error" />,
        className: 'ci-check-failed',
      };
    case 'NEUTRAL':
    case 'SKIPPED':
      return {
        icon: <Icon icon="dash" />,
        className: 'ci-check-neutral',
      };
    case 'CANCELLED':
    case 'TIMED_OUT':
      return {
        icon: <Icon icon="warning" />,
        className: 'ci-check-cancelled',
      };
    default:
      return {
        icon: <Icon icon="question" />,
        className: 'ci-check-unknown',
      };
  }
}
