/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from './types';

import * as stylex from '@stylexjs/stylex';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {T} from './i18n';
import {worktreesForCommit} from './worktrees';

const styles = stylex.create({
  indicator: {
    display: 'inline-flex',
    alignItems: 'center',
    color: 'var(--graphite-accent, #4a90e2)',
    marginLeft: 'var(--halfpad)',
  },
  pathList: {
    fontFamily: 'monospace',
    fontSize: '0.9em',
    whiteSpace: 'pre',
    marginTop: 'var(--halfpad)',
  },
});

/**
 * Shows a folder icon when a commit is checked out in one or more worktrees.
 * Hovering shows the paths of the worktrees.
 */
export function WorktreeIndicator({hash}: {hash: Hash}) {
  const worktrees = useAtomValue(worktreesForCommit(hash));

  if (!worktrees.length) {
    return null;
  }

  const paths = worktrees.map(w => w.path).join('\n');

  return (
    <Tooltip
      title={
        <>
          <T>Checked out in worktree:</T>
          <div {...stylex.props(styles.pathList)}>{paths}</div>
        </>
      }>
      <span {...stylex.props(styles.indicator)}>
        <Icon icon="folder-opened" />
      </span>
    </Tooltip>
  );
}
