/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Tooltip} from './Tooltip';
import {T} from './i18n';
import platform from './platform';
import {useEffect, useState} from 'react';
import {Icon} from 'shared/Icon';

import './Copyable.css';

/** Click to copy text and show a confirmation tooltip */
export function Copyable({children, className}: {children: string; className?: string}) {
  const [showingSuccess, setShowingSuccess] = useState(false);
  useEffect(() => {
    if (showingSuccess) {
      const timeout = setTimeout(() => setShowingSuccess(false), 1500);
      return () => clearTimeout(timeout);
    }
  }, [showingSuccess, setShowingSuccess]);

  return (
    <Tooltip
      trigger="manual"
      shouldShow={showingSuccess}
      component={CopiedSuccessTooltipContent(children)}>
      <div
        className={'copyable' + (className ? ` ${className}` : '')}
        tabIndex={0}
        onClick={() => {
          platform.clipboardCopy(children);
          setShowingSuccess(true);
        }}>
        {children}
        <Icon icon="copy" />
      </div>
    </Tooltip>
  );
}

function CopiedSuccessTooltipContent(text: string) {
  return () => (
    <span className="copyable-success-tooltip">
      <T replace={{$copiedText: <span className="copyable-success-overflow">{text}</span>}}>
        Copied '$copiedText'.
      </T>
    </span>
  );
}
