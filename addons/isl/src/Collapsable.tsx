/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useState} from 'react';
import {Icon} from 'shared/Icon';
import './Collapsable.css';

export function Collapsable({
  startExpanded,
  children,
  title,
  className,
}: {
  startExpanded?: boolean;
  children: React.ReactNode;
  title: React.ReactNode;
  className?: string;
}) {
  const [isExpanded, setIsExpanded] = useState(startExpanded === true);
  return (
    <div className={'collapsable' + (className ? ` ${className}` : '')}>
      <div className="collapsable-title" onClick={() => setIsExpanded(!isExpanded)}>
        <Icon icon={isExpanded ? 'chevron-down' : 'chevron-right'} /> {title}
      </div>
      {isExpanded ? <div className="collapsable-contents">{children}</div> : null}
    </div>
  );
}
