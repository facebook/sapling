/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Result} from '../types';
import type {MutableRefObject} from 'react';
import type {Comparison} from 'shared/Comparison';

import {ErrorNotice} from '../ErrorNotice';
import {Internal} from '../Internal';
import {ThoughtBubbleIcon} from '../ThoughtBubbleIcon';
import {Tooltip} from '../Tooltip';
import {tracker as originalTracker} from '../analytics';
import {useFeatureFlagSync} from '../featureFlags';
import {T, t} from '../i18n';
import {uncommittedChangesWithPreviews} from '../previews';
import {commitByHash} from '../serverAPIState';
import {commitInfoViewCurrentCommits, commitMode, editedCommitMessages} from './CommitInfoState';
import {getInnerTextareaForVSCodeTextArea} from './utils';
import {VSCodeButton, VSCodeTextArea} from '@vscode/webview-ui-toolkit/react';
import {
  selectorFamily,
  useRecoilCallback,
  useRecoilRefresher_UNSTABLE,
  useRecoilValue,
  useRecoilValueLoadable,
} from 'recoil';
import {ComparisonType} from 'shared/Comparison';
import {Icon} from 'shared/Icon';
import {useThrottledEffect} from 'shared/hooks';
import {unwrap} from 'shared/utils';

import './GenerateWithAI.css';

// We want to log the user id for all generated AI events,
// use this special tracker to do it. But we can't make this null, or else tracker.operation won't run.
const tracker = Internal?.trackerWithUserInfo ?? originalTracker;

/** Either a commit hash or "commit/aaaaa" when making a new commit on top of hash aaaaa  */
type HashKey = `commit/${string}` | string;

export function GenerateAICommitMesageButton({
  textAreaRef,
  appendToTextArea,
}: {
  textAreaRef: MutableRefObject<unknown>;
  appendToTextArea: (toAdd: string) => unknown;
}) {
  const currentCommit = useRecoilValue(commitInfoViewCurrentCommits)?.[0];
  const mode = useRecoilValue(commitMode);
  const featureEnabled = useFeatureFlagSync(Internal.featureFlags?.GeneratedAICommitMessages);

  useThrottledEffect(
    () => {
      if (currentCommit != null && featureEnabled) {
        tracker.track('GenerateAICommitMessageButtonImpression');
      }
    },
    100,
    [currentCommit?.hash, mode, featureEnabled],
  );

  const hashKey: HashKey | undefined =
    currentCommit == null
      ? undefined
      : mode === 'commit'
      ? `commit/${currentCommit.hash}`
      : currentCommit.hash;
  const onDismiss = useRecoilCallback(
    ({snapshot}) =>
      () => {
        if (hashKey != null) {
          const content = snapshot.getLoadable(generatedCommitMessages(hashKey));
          if (content.state !== 'hasValue') {
            tracker.track('DismissGeneratedAICommitMessageModal');
          }
        }
      },
    [hashKey],
  );

  if (hashKey == null || !featureEnabled) {
    return null;
  }
  return (
    <span key="generate-ai-commit-message-button">
      <Tooltip
        trigger="click"
        placement="bottom"
        component={(dismiss: () => void) => (
          <GenerateAICommitMessageModal
            dismiss={dismiss}
            hashKey={hashKey}
            textArea={getInnerTextareaForVSCodeTextArea(textAreaRef.current as HTMLElement)}
            appendToTextArea={appendToTextArea}
          />
        )}
        onDismiss={onDismiss}
        title={t('Generate a commit message suggestion with AI')}>
        <VSCodeButton appearance="icon" data-testid="generate-commit-message-button">
          <ThoughtBubbleIcon />
        </VSCodeButton>
      </Tooltip>
    </span>
  );
}

const cachedSuggestions = new Map<
  string,
  {lastFetch: number; messagePromise: Promise<Result<string>>}
>();
const ONE_HOUR = 60 * 60 * 1000;
const MAX_SUGGESTION_CACHE_AGE = 24 * ONE_HOUR; // cache aggressively since we have an explicit button to invalidate
const generatedCommitMessages = selectorFamily<Result<string>, HashKey>({
  key: 'generatedCommitMessages',
  get:
    (hashKey: string | undefined) =>
    ({get}) => {
      if (hashKey == null || Internal.generateAICommitMessage == null) {
        return Promise.resolve({value: ''});
      }

      const cached = cachedSuggestions.get(hashKey);
      if (cached && Date.now() - cached.lastFetch < MAX_SUGGESTION_CACHE_AGE) {
        return cached.messagePromise;
      }

      const fileChanges = [];
      if (hashKey === 'head') {
        const uncommittedChanges = get(uncommittedChangesWithPreviews);
        fileChanges.push(...uncommittedChanges.slice(0, 10).map(change => change.path));
      } else {
        const commit = get(commitByHash(hashKey));
        if (commit?.isHead) {
          const uncommittedChanges = get(uncommittedChangesWithPreviews);
          fileChanges.push(...uncommittedChanges.slice(0, 10).map(change => change.path));
        }
        fileChanges.push(...(commit?.filesSample.slice(0, 10).map(change => change.path) ?? []));
      }

      const hashOrHead = hashKey.startsWith('commit/') ? 'head' : hashKey;
      const editedFields = get(editedCommitMessages(hashOrHead));
      const latestWrittenTitle =
        editedFields.type === 'optimistic' ? '(none)' : (editedFields.fields.Title as string);

      const resultPromise = tracker.operation(
        'GenerateAICommitMessage',
        'FetchError',
        undefined,
        async () => {
          const comparison: Comparison = hashKey.startsWith('commit/')
            ? {type: ComparisonType.UncommittedChanges}
            : {type: ComparisonType.Committed, hash: hashKey};
          const response = await unwrap(Internal.generateAICommitMessage)({
            comparison,
            title: latestWrittenTitle,
          });

          return response;
        },
      );

      cachedSuggestions.set(hashKey, {
        lastFetch: Date.now(),
        messagePromise: resultPromise,
      });

      return resultPromise;
    },
});

function GenerateAICommitMessageModal({
  hashKey,
  dismiss,
  appendToTextArea,
}: {
  hashKey: HashKey;
  textArea: HTMLElement | null;
  dismiss: () => unknown;
  appendToTextArea: (toAdd: string) => unknown;
}) {
  const content = useRecoilValueLoadable(generatedCommitMessages(hashKey));
  const refetch = useRecoilRefresher_UNSTABLE(generatedCommitMessages(hashKey));

  const error = content.state === 'hasError' ? content.errorOrThrow() : content.valueMaybe()?.error;

  return (
    <div className="generated-ai-commit-message-modal">
      <VSCodeButton appearance="icon" className="dismiss-modal" onClick={dismiss}>
        <Icon icon="x" />
      </VSCodeButton>
      <b>Generate Summary</b>
      {error ? (
        <ErrorNotice error={error} title={t('Unable to generate commit message')}></ErrorNotice>
      ) : (
        <div className="generated-message-textarea-container">
          <VSCodeTextArea readOnly value={content.valueMaybe()?.value ?? ''} rows={14} />
          {content.state === 'loading' && <Icon icon="loading" />}
        </div>
      )}
      <div className="generated-message-button-bar">
        <VSCodeButton
          disabled={content.state === 'loading' || error != null}
          appearance="secondary"
          onClick={() => {
            tracker.track('RetryGeneratedAICommitMessage');
            cachedSuggestions.delete(hashKey); // make sure we don't re-use cached value
            refetch();
          }}>
          <Icon icon="refresh" slot="start" />
          <T>Try Again</T>
        </VSCodeButton>
        <VSCodeButton
          disabled={content.state === 'loading' || error != null}
          onClick={() => {
            const value = content.state === 'hasValue' ? content.valueOrThrow().value : null;
            if (value) {
              appendToTextArea(value);
            }
            tracker.track('AcceptGeneratedAICommitMessage');
            dismiss();
          }}>
          <Icon icon="check" slot="start" />
          <T>Insert into Summary</T>
        </VSCodeButton>
      </div>
    </div>
  );
}
