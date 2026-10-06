/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from './dag/dag';
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
import {dagWithPreviews} from './previews';
import {succeedableRevset} from './types';

/**
 * For a given obsolete commit, returns info about its orphaned (non-obsolete)
 * children and the latest successor to rebase them onto, or null if the button
 * should not be shown.
 */
function orphanedChildrenForCommit(dag: Dag, hash: Hash) {
  const commit = dag.get(hash);
  if (commit == null || commit.successorInfo == null) {
    return null;
  }

  const successorType = commit.successorInfo.type;

  // Follow the successor chain to find the latest non-obsolete successor.
  const successors = dag.followSuccessors(hash);
  // followSuccessors returns a set; remove the original hash to get just successors.
  const successorHashes = successors
    .toHashes()
    .toArray()
    .filter(h => h !== hash);
  if (successorHashes.length === 0) {
    return null;
  }

  // Pick the first successor. If it's not in the DAG or is itself obsolete, bail out.
  const successorHash = successorHashes[0];
  const successorCommit = dag.get(successorHash);
  if (successorCommit == null || successorCommit.successorInfo != null) {
    return null;
  }

  // Find non-obsolete children of this obsolete commit.
  const children = dag.children(hash);
  const nonObsoleteChildren = dag.nonObsolete(children);
  if (nonObsoleteChildren.size === 0) {
    return null;
  }

  return {
    orphanedChildren: nonObsoleteChildren.toHashes().toArray(),
    successorHash,
    isLanded: successorType === 'land' || successorType === 'pushrebase',
  };
}

/**
 * For a given stack root hash, aggregates all orphaned children across all
 * obsolete commits in the stack. Returns the list of rebase operations needed
 * (each mapping an orphaned child to its target successor), or null if no
 * orphaned commits exist in the stack.
 */
export const orphanedChildrenForStack = atomFamilyWeak((hash: Hash) =>
  atom(get => {
    const dag = get(dagWithPreviews);
    const stackHashes = dag.descendants(hash).toHashes().toArray();

    const rebaseEntries: Array<{orphanedChild: Hash; successorHash: Hash; isLanded: boolean}> = [];
    const allOrphaned: Hash[] = [];
    const allSuccessors: Hash[] = [];

    for (const h of stackHashes) {
      const info = orphanedChildrenForCommit(dag, h);
      if (info != null) {
        for (const child of info.orphanedChildren) {
          rebaseEntries.push({
            orphanedChild: child,
            successorHash: info.successorHash,
            isLanded: info.isLanded,
          });
          allOrphaned.push(child);
        }
        if (!allSuccessors.includes(info.successorHash)) {
          allSuccessors.push(info.successorHash);
        }
      }
    }

    if (rebaseEntries.length === 0) {
      return null;
    }

    return {
      rebaseEntries,
      allOrphaned,
      allSuccessors,
      isLanded: rebaseEntries.every(e => e.isLanded),
    };
  }),
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
