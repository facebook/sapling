/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Icon} from 'isl-components/Icon';
import {t} from './i18n';
import {copyAndShowToast} from './toast';

import './Copyable.css';

/** Click to copy text and show the result in a toast. */
export function Copyable({
  children,
  className,
  iconOnly,
}: {
  children: string;
  className?: string;
  iconOnly?: boolean;
}) {
  const copy = () => {
    void copyAndShowToast(children);
  };

  return (
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
  );
}
