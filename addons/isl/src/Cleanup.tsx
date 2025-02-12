/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UICodeReviewProvider} from './codeReview/UICodeReviewProvider';
import type {CommitInfo, DiffSummary, Hash} from './types';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {nullthrows} from 'shared/utils';
import {OperationDisabledButton} from './OperationDisabledButton';
import {allDiffSummaries, codeReviewProvider} from './codeReview/CodeReviewInfo';
import {t, T} from './i18n';
import {HideOperation} from './operations/HideOperation';
import {useRunOperation} from './operationsState';
import {type Dag, dagWithPreviews} from './previews';
import {latestSuccessorUnlessExplicitlyObsolete} from './successionUtils';

export function isStackEligibleForCleanup(
  hash: Hash,
  dag: Dag,
  diffMap: Map<string, DiffSummary>,
  provider: UICodeReviewProvider,
): boolean {
  return dag
    .descendants(hash)
    .toSeq()
    .every(h => {
      const info = dag.get(h);
      // don't allow hiding a stack you're checked out on
      if (info == null || info.isDot) {
        return false;
      }
      // allow clean up obsoleted commits regardless of review state
      if (info.successorInfo != null) {
        return true;
      }
      // if not obsoleted, still allow cleanup for "Closed" diffs
      if (info.diffId != null) {
        const diffInfo = diffMap.get(info.diffId);
        if (diffInfo != null && provider.isDiffEligibleForCleanup(diffInfo)) {
          return true;
        }
      }
      // no cleanup otherwise
      return false;
    });
}

export function CleanupButton({commit, hasChildren}: {commit: CommitInfo; hasChildren: boolean}) {
  const runOperation = useRunOperation();
  return (
    <Tooltip
      title={
        hasChildren
          ? t('You can safely "clean up" by hiding this stack of commits.')
          : t('You can safely "clean up" by hiding this commit.')
      }
      placement="bottom">
      <Button
        icon
        onClick={() => {
          runOperation(new HideOperation(latestSuccessorUnlessExplicitlyObsolete(commit)));
        }}>
        <Icon icon="eye-closed" slot="start" />
        {hasChildren ? <T>Clean up stack</T> : <T>Clean up</T>}
      </Button>
    </Tooltip>
  );
}

export function CleanupAllButton() {
  const dag = useAtomValue(dagWithPreviews);
  const reviewProvider = useAtomValue(codeReviewProvider);
  const diffMap = useAtomValue(allDiffSummaries)?.value;
  if (diffMap == null || reviewProvider == null) {
    return null;
  }

  const stackBases = dag.roots(dag.draft()).toArray();
  const cleanableStacks = stackBases.filter(hash =>
    isStackEligibleForCleanup(hash, dag, diffMap, reviewProvider),
  );

  const disabled = cleanableStacks.length === 0;
  return (
    <Tooltip
      title={
        disabled
          ? t('No landed or closed commits to hide')
          : t('Hide all commits for landed or closed Diffs')
      }>
      <OperationDisabledButton
        contextKey="cleanup-all"
        runOperation={() => {
          return cleanableStacks.map(hash => {
            const info = nullthrows(dag.get(hash));
            return new HideOperation(latestSuccessorUnlessExplicitlyObsolete(info));
          });
        }}
        icon={<Icon icon="eye-closed" slot="start" />}
        disabled={disabled}>
        <T>Clean up all</T>
      </OperationDisabledButton>
    </Tooltip>
  );
}
