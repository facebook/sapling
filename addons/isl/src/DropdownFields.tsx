/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {VSCodeDivider} from '@vscode/webview-ui-toolkit/react';
import {Icon} from 'shared/Icon';

import './DropdownFields.css';

export function DropdownFields({
  title,
  icon,
  children,
  className,
  ...rest
}: {
  title: React.ReactNode;
  icon: string;
  children: React.ReactNode;
  'data-testid'?: string;
  className?: string;
}) {
  return (
    <div className={'dropdown-fields' + (className != null ? ` ${className}` : '')} {...rest}>
      <div className="dropdown-fields-header">
        <Icon icon={icon} size="M" />
        <strong role="heading">{title}</strong>
      </div>
      <VSCodeDivider />
      <div className="dropdown-fields-content">{children}</div>
    </div>
  );
}

export function DropdownField({
  title,
  children,
}: {
  title: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="dropdown-field">
      <strong>{title}</strong>
      <div>{children}</div>
    </div>
  );
}
