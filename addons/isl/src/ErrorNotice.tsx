/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Icon} from './Icon';
import React, {useState, Component} from 'react';

import './error-notice.css';

export function ErrorNotice({
  title,
  error,
  buttons,
}: {
  title: React.ReactNode;
  error: Error;
  buttons?: Array<React.ReactNode>;
}) {
  const [isExpanded, setIsExpanded] = useState(false);
  return (
    <div className="error-notice" onClick={() => setIsExpanded(e => !e)}>
      <div className="error-notice-left">
        <div className="error-notice-arrow">
          <Icon icon={isExpanded ? 'triangle-down' : 'triangle-right'} />
        </div>
        <div className="error-notice-content">
          <span className="error-notice-title">{title}</span>
          <span className="error-notice-byline">{error.message}</span>
          {isExpanded ? <span className="error-notice-stack-trace">{error.stack}</span> : null}
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
