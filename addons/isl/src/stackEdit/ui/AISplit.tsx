/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitRev, CommitStackState} from '../commitStackState';
import type {DiffCommit, PartiallySelectedDiffCommit} from '../diffSplitTypes';
import {
  bumpStackEditMetric,
  findStartEndRevs,
  SplitRangeRecord,
  type UseStackEditState,
} from './stackEditState';

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {ButtonWithDropdownTooltip} from 'isl-components/ButtonWithDropdownTooltip';
import {InlineErrorBadge} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {type ForwardedRef, forwardRef, useEffect, useRef, useState} from 'react';
import {randomId} from 'shared/utils';
import {MinHeightTextField} from '../../CommitInfoView/MinHeightTextField';
import {Column} from '../../ComponentUtils';
import {useGeneratedFileStatuses} from '../../GeneratedFile';
import {Internal} from '../../Internal';
import {tracker} from '../../analytics';
import {useFeatureFlagSync} from '../../featureFlags';
import {t, T} from '../../i18n';
import {GeneratedStatus} from '../../types';
import {applyDiffSplit, diffCommit} from '../diffSplit';
import {next} from '../revMath';

const styles = stylex.create({
  full: {
    minWidth: '300px',
    width: '100%',
  },
});

type AISplitButtonProps = {
  stackEdit: UseStackEditState;
  commitStack: CommitStackState;
  subStack: CommitStackState;
  rev: CommitRev;
};

type AISplitButtonLoadingState =
  | {type: 'READY'}
  | {type: 'LOADING'; id: string}
  | {type: 'ERROR'; error: Error};

export const AISplitButton = forwardRef(
  (
    {stackEdit, commitStack, subStack, rev}: AISplitButtonProps,
    ref: ForwardedRef<HTMLButtonElement>,
  ) => {
    const {splitCommitWithAI} = Internal;
    const enableAICommitSplit =
      useFeatureFlagSync(Internal.featureFlags?.AICommitSplit) && splitCommitWithAI != null;

    const [loadingState, setLoadingState] = useState<AISplitButtonLoadingState>({type: 'READY'});

    // Reset state if commitStack changes while in LOADING state. E.g., user manually updated commits locally.
    useEffect(() => {
      if (loadingState.type === 'LOADING') {
        setLoadingState({type: 'READY'});
      }
      return () => {
        // Cancel loading state when unmounted
        setLoadingState({type: 'READY'});
      };
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [commitStack]); // Triggered when commitStack changes

    const applyNewDiffSplitCommits = (
      subStack: CommitStackState,
      rev: CommitRev,
      commits: ReadonlyArray<PartiallySelectedDiffCommit>,
    ) => {
      const [startRev, endRev] = findStartEndRevs(stackEdit);
      if (startRev != null && endRev != null) {
        // Replace the current, single rev with the new stack, which might have multiple revs.
        const newSubStack = applyDiffSplit(subStack, rev, commits);
        // Replace the [start, end+1] range with the new stack in the commit stack.
        const newCommitStack = commitStack.applySubStack(startRev, next(endRev), newSubStack);
        // Find the new split range.
        const endOffset = newCommitStack.size - commitStack.size;
        const startKey = newCommitStack.get(rev)?.key ?? '';
        const endKey = newCommitStack.get(next(rev, endOffset))?.key ?? '';
        const splitRange = SplitRangeRecord({startKey, endKey});
        // Update the main stack state.
        stackEdit.push(newCommitStack, {name: 'splitWithAI'}, splitRange);
      }
    };

    const diffWithoutGeneratedFiles = useDiffWithoutGeneratedFiles(subStack, rev);

    const [guidanceToAI, setGuidanceToAI] = useState('');
    const fetch = async () => {
      if (loadingState.type === 'LOADING' || splitCommitWithAI == null) {
        return;
      }
      if (diffWithoutGeneratedFiles.files.length === 0) {
        return;
      }

      bumpStackEditMetric('clickedAiSplit');

      const id = randomId();
      setLoadingState({type: 'LOADING', id});
      const args =
        guidanceToAI == null || guidanceToAI.trim() === ''
          ? {}
          : {
              user_prompt: guidanceToAI.trim(),
            };

      try {
        const result: ReadonlyArray<PartiallySelectedDiffCommit> = await tracker.operation(
          'AISplitButtonClick',
          'SplitSuggestionError',
          undefined,
          () => splitCommitWithAI(diffWithoutGeneratedFiles, args),
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
          <Tooltip title={t('Split this commit using AI')} placement="bottom">
            <ButtonWithDropdownTooltip
              label={<T>AI Split</T>}
              data-testid="cwd-dropdown-button"
              kind="icon"
              icon={<Icon icon="sparkle" />}
              onClick={fetch}
              ref={ref}
              tooltip={
                <DetailsDropdown
                  loadingState={loadingState}
                  submit={fetch}
                  guidanceToAI={guidanceToAI}
                  setGuidanceToAI={setGuidanceToAI}
                />
              }
            />
          </Tooltip>
        );
      case 'LOADING':
        return (
          <Tooltip title={t('Split is working, click to cancel')} placement="bottom">
            <ButtonWithDropdownTooltip
              label={<T>Splitting</T>}
              data-testid="cwd-dropdown-button"
              kind="icon"
              icon={<Icon icon="loading" />}
              onClick={cancel}
              tooltip={
                <DetailsDropdown
                  loadingState={loadingState}
                  submit={cancel}
                  guidanceToAI={guidanceToAI}
                  setGuidanceToAI={setGuidanceToAI}
                />
              }
            />
          </Tooltip>
        );
      case 'ERROR':
        return (
          <Column alignStart>
            <ButtonWithDropdownTooltip
              label={<T>AI Split</T>}
              data-testid="cwd-dropdown-button"
              kind="icon"
              icon={<Icon icon="sparkle" />}
              onClick={fetch}
              tooltip={
                <DetailsDropdown
                  loadingState={loadingState}
                  submit={fetch}
                  guidanceToAI={guidanceToAI}
                  setGuidanceToAI={setGuidanceToAI}
                />
              }
            />
            <InlineErrorBadge error={loadingState.error} placement="bottom">
              <T>AI Split Failed</T>
            </InlineErrorBadge>
          </Column>
        );
    }
  },
);

function useDiffWithoutGeneratedFiles(subStack: CommitStackState, rev: CommitRev): DiffCommit {
  const diffForAllFiles = diffCommit(subStack, rev);
  const allFilePaths = diffForAllFiles.files.map(f => f.bPath);
  const generatedFileStatuses = useGeneratedFileStatuses(allFilePaths);
  const filesWithoutGeneratedFiles = diffForAllFiles.files.filter(
    f => generatedFileStatuses[f.bPath] !== GeneratedStatus.Generated,
  );
  return {
    ...diffForAllFiles,
    files: filesWithoutGeneratedFiles,
  };
}

function DetailsDropdown({
  loadingState,
  submit,
  guidanceToAI,
  setGuidanceToAI,
}: {
  loadingState: AISplitButtonLoadingState;
  submit: () => unknown;
  guidanceToAI: string;
  setGuidanceToAI: React.Dispatch<React.SetStateAction<string>>;
}) {
  const ref = useRef(null);
  return (
    <Column alignStart>
      <MinHeightTextField
        ref={ref}
        keepNewlines
        containerXstyle={styles.full}
        value={guidanceToAI}
        onInput={e => setGuidanceToAI(e.currentTarget.value)}>
        <T>Provide additional instructions to AI (optional)</T>
      </MinHeightTextField>
      <Button onClick={submit} style={{alignSelf: 'end'}} primary>
        {loadingState.type === 'LOADING' ? <Icon icon="loading" /> : <Icon icon="sparkle" />}
        {loadingState.type === 'LOADING' ? <T>Splitting</T> : <T>AI Split</T>}
      </Button>
    </Column>
  );
}
