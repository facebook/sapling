/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from './types';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtomValue} from 'jotai';
import {HighlightCommitsWhileHovering} from './HighlightedCommits';
import {Internal} from './Internal';
import {tracker} from './analytics';
import {useFeatureFlagSync} from './featureFlags';
import {t, T} from './i18n';
import {atomFamilyWeak} from './jotaiUtils';
import {RebaseOperation} from './operations/RebaseOperation';
import {useRunOperation} from './operationsState';
import {orphanedChildrenInStack} from './orphanedCommits';
import {dagWithPreviews} from './previews';
import {succeedableRevset} from './types';

export const orphanedChildrenForStack = atomFamilyWeak((hash: Hash) =>
  atom(get => orphanedChildrenInStack(get(dagWithPreviews), hash)),
);

export function RebaseOrphanedStackButton({hash}: {hash: Hash}) {
  const featureEnabled = useFeatureFlagSync(Internal.featureFlags?.RebaseOntoSuccessor);
  const info = useAtomValue(orphanedChildrenForStack(hash));
  const runOperation = useRunOperation();

  if (!featureEnabled) {
    return null;
  }

  if (info == null) {
    return null;
  }

  const {rebaseEntries, allOrphaned, allSuccessors, isLanded} = info;

  const handleClick = () => {
    tracker.track('ClickRebaseOntoSuccessor', {
      extras: {
        sources: allOrphaned,
        dest: allSuccessors.join(','),
        numOrphans: allOrphaned.length,
        isLanded,
      },
    });
    for (const {orphanedChild, successorHash} of rebaseEntries) {
      runOperation(
        new RebaseOperation(succeedableRevset(orphanedChild), succeedableRevset(successorHash)),
      );
    }
  };

  return (
    <HighlightCommitsWhileHovering toHighlight={[...allOrphaned, ...allSuccessors]}>
      <Tooltip
        title={
          isLanded
            ? t('Rebase orphaned commits onto the landed successors of their obsolete parents')
            : t('Rebase orphaned commits onto the latest successors of their obsolete parents')
        }
        placement="bottom">
        <Button icon onClick={handleClick} data-testid="rebase-onto-successor-button">
          <Icon icon="git-pull-request" slot="start" />
          <T>{isLanded ? 'Rebase onto landed' : 'Rebase orphaned commits'}</T>
        </Button>
      </Tooltip>
    </HighlightCommitsWhileHovering>
  );
}
