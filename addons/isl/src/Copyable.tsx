/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useEffect, useState} from 'react';
import {t, T} from './i18n';
import platform from './platform';

import './Copyable.css';

/** Click to copy text and show a confirmation tooltip. If content is provided, use that instead of  */
export function Copyable({
  children,
  className,
  iconOnly,
}: {
  children: string;
  className?: string;
  iconOnly?: boolean;
}) {
  const [showingSuccess, setShowingSuccess] = useState(false);
  const copy = () => {
    platform.clipboardCopy(children);
    setShowingSuccess(true);
  };
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
        role="button"
        className={
          'copyable' + (className ? ` ${className}` : '') + (iconOnly === true ? ' icon-only' : '')
        }
        tabIndex={0}
        aria-label={iconOnly === true ? t('Copy $value', {replace: {$value: children}}) : undefined}
        onKeyDown={e => {
          if (e.key === 'Enter' || e.key === ' ') {
            copy();
            e.preventDefault();
            e.stopPropagation();
          }
        }}
        onClick={e => {
          copy();
          e.preventDefault();
          e.stopPropagation();
        }}>
        {iconOnly !== true && children}
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
