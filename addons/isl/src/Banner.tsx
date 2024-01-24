/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {Tooltip} from './Tooltip';

import './Banner.css';

export enum BannerKind {
  default = 'default',
  warning = 'warning',
  error = 'error',
  green = 'green',
}

export function Banner({
  kind,
  children,
  icon,
  buttons,
  tooltip,
}: {
  kind?: BannerKind;
  children: ReactNode;
  icon?: ReactNode;
  buttons?: ReactNode;
  tooltip?: string;
}) {
  const content = (
    <div className={`banner banner-${kind ?? 'default'}`}>
      <div className="banner-content">
        {icon ?? null} {children}
      </div>
      {buttons && <div className="banner-buttons">{buttons}</div>}
    </div>
  );
  if (tooltip) {
    return (
      <Tooltip trigger="hover" placement="bottom" title={tooltip}>
        {content}
      </Tooltip>
    );
  }
  return content;
}
