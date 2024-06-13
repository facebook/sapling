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
  alwaysShowButtons,
}: {
  kind?: BannerKind;
  children: ReactNode;
  icon?: ReactNode;
  buttons?: ReactNode;
  alwaysShowButtons?: boolean;
}) {
  return (
    <div className={`banner banner-${kind ?? 'default'}`}>
      <div className="banner-content">
        {icon ?? null} {children}
      </div>
      {buttons && (
        <div className={'banner-buttons' + (alwaysShowButtons ? ' banner-buttons-visible' : '')}>
          {buttons}
        </div>
      )}
    </div>
  );
}

export function BannerTooltip({tooltip, children}: {tooltip: string; children: ReactNode}) {
  return (
    <Tooltip trigger="hover" placement="bottom" title={tooltip}>
      {children}
    </Tooltip>
  );
}
