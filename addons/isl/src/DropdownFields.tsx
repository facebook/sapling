/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Divider} from './components/Divider';
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
      <Divider />
      <div className="dropdown-fields-content">{children}</div>
    </div>
  );
}

export function DropdownField({
  title,
  children,
  ...rest
}: {
  title: React.ReactNode;
  children: React.ReactNode;
} & Omit<React.DetailedHTMLProps<React.HTMLAttributes<HTMLDivElement>, HTMLDivElement>, 'title'>) {
  return (
    <div className="dropdown-field">
      <strong>{title}</strong>
      <div {...rest}>{children}</div>
    </div>
  );
}
