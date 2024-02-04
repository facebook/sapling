/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Placement} from './Tooltip';

import {Tooltip} from './Tooltip';
import {t} from './i18n';

type CommitTitleProps = React.HTMLAttributes<HTMLDivElement> & {
  commitMessage: string;
  tooltipPlacement?: Placement;
};

/** One line commit message with tooltip about full message. */
export function CommitTitle(props: CommitTitleProps) {
  const {commitMessage, tooltipPlacement, ...restProps} = props;
  const title = commitMessage.split('\n')[0];
  const trimmed = commitMessage.trim();
  if (trimmed === '') {
    return null;
  }

  const divElement = <div {...restProps}>{title}</div>;
  if (title === trimmed) {
    // No need to use a tooltip.
    return divElement;
  } else {
    return (
      <Tooltip placement={tooltipPlacement} title={title}>
        {divElement}
      </Tooltip>
    );
  }
}

export function temporaryCommitTitle() {
  return t('Temporary Commit at $time', {replace: {$time: new Date().toLocaleString()}});
}
