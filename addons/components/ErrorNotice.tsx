/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import React, {Component, useState} from 'react';
import {Icon} from './Icon';
import type {Placement} from './Tooltip';
import {Tooltip} from './Tooltip';

import './error-notice.css';

export function ErrorNotice({
  title,
  description,
  details,
  error,
  buttons,
  startExpanded,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  /** Hidden details shown when expanded */
  details?: React.ReactNode;
  error?: Error;
  buttons?: Array<React.ReactNode>;
  startExpanded?: boolean;
}) {
  const [isExpanded, setIsExpanded] = useState(startExpanded === true);
  return (
    <div className="error-notice" onClick={() => setIsExpanded(e => !e)}>
      <div className="error-notice-left">
        <div className="error-notice-arrow">
          <Icon icon={isExpanded ? 'triangle-down' : 'triangle-right'} />
        </div>
        <div className="error-notice-content">
          <span className="error-notice-title">{title}</span>
          <span className="error-notice-byline">{description ?? error?.message}</span>
          {isExpanded ? (
            <div
              className="error-notice-details"
              onClick={e => {
                // don't close the notice when clicking/text selecting the details
                e.stopPropagation();
              }}>
              {details}
              {error != null && (
                <span className="error-notice-stack-trace">{error.stack ?? error.message}</span>
              )}
            </div>
          ) : null}
        </div>
      </div>
      {buttons ? <div className="error-notice-buttons">{buttons}</div> : null}
    </div>
  );
}

type Props = {
  children: React.ReactNode;
};

type State = {error: Error | null};

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = {error: null};
  }

  static getDerivedStateFromError(error: Error) {
    return {error};
  }

  render() {
    if (this.state.error != null) {
      return <ErrorNotice title="Something went wrong" error={this.state.error} />;
    }

    return this.props.children;
  }
}

/**
 * One-line error message, which shows the full ErrorNotice in a tooltip on hover.
 */
export function InlineErrorBadge({
  children,
  error,
  title,
  placement,
}: {
  children: ReactNode;
  error: Error;
  title?: ReactNode;
  placement?: Placement;
}) {
  return (
    <div className="inline-error-badge">
      <Tooltip component={TooltipErrorDetails(error, title)} placement={placement}>
        <Icon icon="error" slot="start" />
        <span>{children}</span>
      </Tooltip>
    </div>
  );
}

function TooltipErrorDetails(error: Error, title?: ReactNode) {
  return function Component() {
    return (
      <div className="inline-error-tooltip">
        <ErrorNotice title={title ?? error.message} error={error} startExpanded />
      </div>
    );
  };
}
