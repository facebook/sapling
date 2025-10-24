/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {randomId} from 'shared/utils';
import {tracker} from '../analytics';
import {diffCommentData} from '../codeReview/codeReviewAtoms';
import {diffSummary} from '../codeReview/CodeReviewInfo';
import {Internal} from '../Internal';
import {readAtom} from '../jotaiUtils';
import type {DiffComment} from '../types';
import {assert} from '../utils';
import type {SmartActionConfig} from './types';

function hasDiffFailedSignals(diffId: string): boolean {
  const diffSummaryResult = readAtom(diffSummary(diffId));
  return (
    diffSummaryResult.error == null &&
    diffSummaryResult.value?.signalSummary != null &&
    diffSummaryResult.value.signalSummary === 'failed'
  );
}

export const smartActionsConfig: SmartActionConfig[] = [
  // Auto-split commit
  // TODO: Implement

  // Fill commit info
  {
    id: 'fill-commit-info',
    label: 'Fill commit info',
    trackEventName: 'FillCommitMessage',
    featureFlag: 'AIGenerateCommitMessage',
    platformRestriction: ['vscode'],
    getMessagePayload: () => ({
      type: 'platform/fillCommitMessageWithAI',
      id: randomId(),
      source: 'smartAction',
    }),
    shouldShow: context => !context.conflicts && !context.commit, // Only for uncommitted changes
  },

  // Validate changes
  {
    id: 'validate-changes',
    label: 'Validate changes',
    trackEventName: 'ValidateChanges',
    featureFlag: 'AIValidateChanges',
    platformRestriction: ['vscode'],
    getMessagePayload: () => ({
      type: 'platform/validateChangesWithAI',
    }),
    shouldShow: context => !context.conflicts && !context.commit, // Only for uncommitted changes
  },

  // Generate tests
  {
    id: 'generate-tests',
    label: 'Generate tests for changes',
    trackEventName: 'GenerateTests',
    featureFlag: 'AIGenerateTestsForModifiedCode',
    platformRestriction: ['vscode'],
    getMessagePayload: () => ({
      type: 'platform/createTestForModifiedCodeWithAI',
    }),
    shouldShow: context => !context.conflicts,
  },

  // Review code
  {
    id: 'review-code',
    label: 'Review code',
    trackEventName: 'ReviewCommit',
    featureFlag: 'AICodeReviewUpsell',
    platformRestriction: ['vscode'],
    getMessagePayload: context => ({
      type: 'platform/runAICodeReviewChat',
      source: 'smartAction',
      reviewScope: context.commit ? 'current commit' : 'uncommitted changes',
    }),
    shouldShow: context => !context.conflicts,
  },

  // Resolve comments
  {
    id: 'resolve-comments',
    label: 'Resolve all comments',
    trackEventName: 'ResolveAllComments',
    featureFlag: 'InlineCommentAIResolve',
    platformRestriction: ['vscode'],
    getMessagePayload: context => {
      const diffId = context.commit?.diffId;
      assert(diffId != null, 'Diff ID is required for resolving comments');

      const diffComments = readAtom(diffCommentData(diffId));
      let comments: DiffComment[] = [];
      if (diffComments.state === 'hasData') {
        comments = diffComments.data;
      }

      return {
        type: 'platform/resolveAllCommentsWithAI',
        diffId,
        comments,
        filePaths: [...(context.commit?.filePathsSample ?? [])],
        repoPath: context.repoPath,
      };
    },
    shouldShow: context => {
      if (context.conflicts != null) {
        return false;
      }
      const diffId = context.commit?.diffId;
      if (diffId == null) {
        return false;
      }
      const diffComments = readAtom(diffCommentData(diffId));
      return diffComments.state === 'hasData' && diffComments.data.length > 0;
    },
  },

  // Resolve failed signals
  {
    id: 'resolve-failed-signals',
    label: 'Fix failed signals',
    trackEventName: 'ResolveFailedSignals',
    featureFlag: 'AIResolveFailedSignals',
    platformRestriction: ['vscode'],
    getMessagePayload: context => {
      const commit = context.commit;
      assert(commit != null, 'Commit is required for resolving failed signals');

      const diffId = commit.diffId;
      assert(diffId != null, 'Diff ID is required for resolving failed signals');
      assert(hasDiffFailedSignals(diffId), 'Diff must have failed signals to resolve');

      const diffSummaryResult = readAtom(diffSummary(diffId));
      const diffVersionNumber = Internal.getDiffVersionNumber?.(
        diffSummaryResult.value,
        commit.hash,
      );
      assert(diffVersionNumber != null, 'Diff version number is required');

      return {
        type: 'platform/resolveFailedSignalsWithAI',
        diffId,
        diffVersionNumber,
        repoPath: context.repoPath,
      };
    },
    shouldShow: context => {
      if (context.conflicts != null) {
        return false;
      }
      const diffId = context.commit?.diffId;
      if (diffId == null) {
        return false;
      }
      return hasDiffFailedSignals(diffId);
    },
  },

  // Resolve merge conflicts
  {
    id: 'resolve-merge-conflicts',
    label: 'Resolve merge conflicts',
    trackEventName: 'ResolveAllConflicts',
    featureFlag: 'AIResolveConflicts',
    platformRestriction: ['vscode'],
    shouldShow: context => context.conflicts != null, // Only for merge conflicts
    getMessagePayload: context => {
      const conflicts = context.conflicts;
      assert(conflicts != null, 'Must be in merge conflict state to resolve conflicts');
      tracker.track('DevmateResolveAllConflicts');
      return {
        type: 'platform/resolveAllConflictsWithAI',
        conflicts,
      };
    },
  },
] satisfies SmartActionConfig[];
