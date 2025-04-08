/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitRev, CommitStackState} from '../commitStackState';
import type {PartiallySelectedDiffCommit} from '../diffSplitTypes';

import {Button} from 'isl-components/Button';
import {InlineErrorBadge} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useEffect, useState} from 'react';
import {randomId} from 'shared/utils';
import {Column} from '../../ComponentUtils';
import {Internal} from '../../Internal';
import {tracker} from '../../analytics';
import {useFeatureFlagSync} from '../../featureFlags';
import {t, T} from '../../i18n';
import {diffCommit} from '../diffSplit';

type AISplitButtonProps = {
  commitStack: CommitStackState;
  subStack: CommitStackState;
  rev: CommitRev;
  applyNewDiffSplitCommits: (
    subStack: CommitStackState,
    rev: CommitRev,
    commits: ReadonlyArray<PartiallySelectedDiffCommit>,
  ) => unknown;
};

type AISplitButtonLoadingState =
  | {type: 'READY'}
  | {type: 'LOADING'; id: string}
  | {type: 'ERROR'; error: Error};

export function AISplitButton({
  commitStack,
  subStack,
  rev,
  applyNewDiffSplitCommits,
}: AISplitButtonProps) {
  const {splitCommitWithAI} = Internal;
  const enableAICommitSplit =
    useFeatureFlagSync(Internal.featureFlags?.AICommitSplit) && splitCommitWithAI != null;

  const [loadingState, setLoadingState] = useState<AISplitButtonLoadingState>({type: 'READY'});

  // Make first commit be emphasized if there's only one commit (size == 2 due to empty right commit)
  const emphasize = rev === 0 && commitStack.size === 2;

  // Reset state if commitStack changes while in LOADING state. E.g., user manually updated commits locally.
  useEffect(() => {
    if (loadingState.type === 'LOADING') {
      setLoadingState({type: 'READY'});
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [commitStack]); // Triggered when commitStack changes

  const fetch = async () => {
    if (loadingState.type === 'LOADING' || splitCommitWithAI == null) {
      return;
    }
    const diff = diffCommit(subStack, rev);
    if (diff.files.length === 0) {
      return;
    }

    const id = randomId();
    setLoadingState({type: 'LOADING', id});
    try {
      const result = await tracker.operation(
        'AISplitButtonClick',
        'SplitSuggestionError',
        undefined,
        () => splitCommitWithAI(diff),
      );
      setLoadingState(prev => {
        if (prev.type === 'LOADING' && prev.id === id) {
          const commits = result.filter(c => c.files.length > 0);
          if (commits.length > 0) {
            applyNewDiffSplitCommits(subStack, rev, commits);
          }
          return {type: 'READY'};
        }
        return prev;
      });
    } catch (err) {
      if (err != null) {
        setLoadingState(prev => {
          if (prev.type === 'LOADING' && prev.id === id) {
            return {type: 'ERROR', error: err as Error};
          }
          return prev;
        });
        return;
      }
    }
  };

  const cancel = () => {
    setLoadingState(prev => {
      const {type} = prev;
      if (type === 'LOADING' || type === 'ERROR') {
        return {type: 'READY'};
      }
      return prev;
    });
  };

  if (!enableAICommitSplit) {
    return null;
  }

  switch (loadingState.type) {
    case 'READY':
      return (
        <Tooltip title={t('Automatically split this commit using AI')} placement="bottom">
          <Button onClick={fetch} icon={!emphasize}>
            <Icon icon="sparkle" />
            <T>AI Split</T>
          </Button>
        </Tooltip>
      );
    case 'LOADING':
      return (
        <Tooltip title={t('Split is working, click to cancel')} placement="bottom">
          <Button onClick={cancel}>
            <Icon icon="loading" />
            <T>Splitting</T>
          </Button>
        </Tooltip>
      );
    case 'ERROR':
      return (
        <Column alignStart>
          <Button onClick={fetch}>
            <Icon icon="sparkle" />
            <T>Split this commit with AI</T>
          </Button>
          <InlineErrorBadge error={loadingState.error} placement="bottom">
            <T>AI Split Failed</T>
          </InlineErrorBadge>
        </Column>
      );
  }
}
