/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import React from 'react';
import {InlineBadge} from './InlineBadge';
import {t} from './i18n';

/** The "(You are here)" blue label. Supports customized styles and children. */
export function YouAreHereLabel(
  props: {
    title?: string;
    /** Rendered inside the blue badge, after the title. */
    badgeChildren?: React.ReactNode;
  } & React.HTMLAttributes<HTMLDivElement>,
) {
  const {title = t('You are here'), badgeChildren, children, ...rest} = props;
  return (
    <div className="you-are-here-container" {...rest}>
      <InlineBadge kind="primary">
        {title}
        {badgeChildren}
      </InlineBadge>
      {children}
    </div>
  );
}
