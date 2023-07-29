/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Result} from '../types';
import type {MutableRefObject} from 'react';

import serverAPI from '../ClientToServerAPI';
import {Internal} from '../Internal';
import {ThoughtBubbleIcon} from '../ThoughtBubbleIcon';
import {Tooltip} from '../Tooltip';
import {tracker} from '../analytics';
import {T, t} from '../i18n';
import {uncommittedChangesWithPreviews} from '../previews';
import {commitByHash} from '../serverAPIState';
import {commitInfoViewCurrentCommits, commitMode} from './CommitInfoState';
import {getInnerTextareaForVSCodeTextArea} from './utils';
import {VSCodeButton, VSCodeTextArea} from '@vscode/webview-ui-toolkit/react';
import {
  selectorFamily,
  useRecoilRefresher_UNSTABLE,
  useRecoilValue,
  useRecoilValueLoadable,
} from 'recoil';
import {Icon} from 'shared/Icon';

import './GenerateWithAI.css';

export function GenerateAICommitMesageButton({
  textAreaRef,
  appendToTextArea,
}: {
  textAreaRef: MutableRefObject<unknown>;
  appendToTextArea: (toAdd: string) => unknown;
}) {
  const currentCommit = useRecoilValue(commitInfoViewCurrentCommits)?.[0];
  const mode = useRecoilValue(commitMode);
  if (currentCommit == null) {
    return null;
  }
  const hashOrHead = mode === 'commit' ? 'head' : currentCommit.hash;
  return (
    <span key="generate-ai-commit-message-button">
      <Tooltip
        trigger="click"
        placement="bottom"
        component={(dismiss: () => void) => (
          <GenerateAICommitMessageModal
            dismiss={dismiss}
            hashOrHead={hashOrHead}
            textArea={getInnerTextareaForVSCodeTextArea(textAreaRef.current as HTMLElement)}
            appendToTextArea={appendToTextArea}
          />
        )}
        title={t('Generate a commit message suggestion with AI')}>
        <VSCodeButton appearance="icon">
          <ThoughtBubbleIcon />
        </VSCodeButton>
      </Tooltip>
    </span>
  );
}

const cachedSuggestions = new Map<string, {lastFetch: number; message: Result<string>}>();
const ONE_HOUR = 60 * 60 * 1000;
const MAX_SUGGESTION_CACHE_AGE = 24 * ONE_HOUR; // cache aggressively since we have an explicit button to invalidate
const generatedCommitMessages = selectorFamily<Result<string>, string>({
  key: 'generatedCommitMessages',
  get:
    (hashOrHead: string | 'head' | undefined) =>
    ({get}) => {
      if (hashOrHead == null || Internal.generateAICommitMessage == null) {
        return {value: ''};
      }

      const cached = cachedSuggestions.get(hashOrHead);
      if (cached) {
        if (Date.now() - cached.lastFetch < MAX_SUGGESTION_CACHE_AGE) {
          return cached.message;
        }
      }

      const fileChanges = [];
      if (hashOrHead === 'head') {
        const uncommittedChanges = get(uncommittedChangesWithPreviews);
        fileChanges.push(...uncommittedChanges.slice(0, 10).map(change => change.path));
      } else {
        const commit = get(commitByHash(hashOrHead));
        if (commit?.isHead) {
          const uncommittedChanges = get(uncommittedChangesWithPreviews);
          fileChanges.push(...uncommittedChanges.slice(0, 10).map(change => change.path));
        }
        fileChanges.push(...(commit?.filesSample.slice(0, 10).map(change => change.path) ?? []));
      }

      return tracker.operation('GenerateAICommitMessage', 'FetchError', undefined, async () => {
        Internal.generateAICommitMessage?.({hashOrHead});

        const response = await serverAPI.nextMessageMatching(
          'generatedAICommitMessage',
          message => message.hashOrHead === hashOrHead,
        );

        cachedSuggestions.set(hashOrHead, {lastFetch: Date.now(), message: response.message});
        return response.message;
      });
    },
});

function GenerateAICommitMessageModal({
  hashOrHead,
  dismiss,
  appendToTextArea,
}: {
  hashOrHead: string;
  textArea: HTMLElement | null;
  dismiss: () => unknown;
  appendToTextArea: (toAdd: string) => unknown;
}) {
  const content = useRecoilValueLoadable(generatedCommitMessages(hashOrHead));
  const refetch = useRecoilRefresher_UNSTABLE(generatedCommitMessages(hashOrHead));

  return (
    <div className="generated-ai-commit-message-modal">
      <VSCodeButton appearance="icon" className="dismiss-modal" onClick={dismiss}>
        <Icon icon="x" />
      </VSCodeButton>
      <b>Generate Summary</b>
      <div className="generated-message-textarea-container">
        <VSCodeTextArea readOnly value={content.valueMaybe()?.value ?? ''} rows={14} />
        {content.state === 'loading' && <Icon icon="loading" />}
      </div>
      <div className="generated-message-button-bar">
        <VSCodeButton
          disabled={content.state === 'loading'}
          appearance="secondary"
          onClick={() => {
            cachedSuggestions.delete(hashOrHead); // make sure we don't re-use cached value
            refetch();
          }}>
          <Icon icon="refresh" slot="start" />
          <T>Try Again</T>
        </VSCodeButton>
        <VSCodeButton
          disabled={content.state === 'loading'}
          onClick={() => {
            const value = content.state === 'hasValue' ? content.valueOrThrow().value : null;
            if (value) {
              appendToTextArea(value);
            }
            dismiss();
          }}>
          <Icon icon="check" slot="start" />
          <T>Insert into Summary</T>
        </VSCodeButton>
      </div>
    </div>
  );
}
