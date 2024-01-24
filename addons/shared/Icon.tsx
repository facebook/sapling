/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import '@vscode/codicons/dist/codicon.css';
import './Icon.css';

export function Icon({
  icon,
  size,
  slot,
  color,
  ...other
}: {
  slot?: 'start';
  icon: string;
  size?: 'S' | 'M' | 'L';
  color?: 'blue' | 'red' | 'green' | 'yellow';
} & React.DetailedHTMLProps<React.HTMLAttributes<HTMLDivElement>, HTMLDivElement>) {
  return (
    <div
      slot={slot}
      className={`codicon codicon-${icon} icon-size-${size ?? 'S'} ${
        color == null ? '' : `icon-${color}`
      }`}
      {...other}
    />
  );
}
