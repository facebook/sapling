/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UICodeReviewProvider} from './codeReview/UICodeReviewProvider';
import type {CommitTreeWithPreviews} from './getCommitTree';
import type {DiffSummary, CommitInfo} from './types';

import {latestSuccessorUnlessExplicitlyObsolete} from './SuccessionTracker';
import {Tooltip} from './Tooltip';
import {t, T} from './i18n';
import {HideOperation} from './operations/HideOperation';
import {useRunOperation} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {Icon} from 'shared/Icon';
import {unwrap} from 'shared/utils';

export function isStackEligibleForCleanup(
  tree: CommitTreeWithPreviews,
  diffMap: Map<string, DiffSummary>,
  provider: UICodeReviewProvider,
): boolean {
  if (
    tree.info.diffId == null ||
    tree.info.isHead || // don't allow hiding a stack you're checked out on
    diffMap.get(tree.info.diffId) == null ||
    !provider.isDiffEligibleForCleanup(unwrap(diffMap.get(tree.info.diffId)))
  ) {
    return false;
  }

  // any child not eligible -> don't show
  for (const subtree of tree.children) {
    if (!isStackEligibleForCleanup(subtree, diffMap, provider)) {
      return false;
    }
  }

  return true;
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
      <VSCodeButton
        appearance="icon"
        onClick={() => {
          runOperation(new HideOperation(latestSuccessorUnlessExplicitlyObsolete(commit)));
        }}>
        <Icon icon="eye-closed" slot="start" />
        {hasChildren ? <T>Clean up stack</T> : <T>Clean up</T>}
      </VSCodeButton>
    </Tooltip>
  );
}
