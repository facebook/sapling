/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FieldConfig, TypeaheadKind, TypeaheadResult} from './types';

import serverApi from '../ClientToServerAPI';
import {Typeahead} from '../components/Typeahead';
import {recentReviewers, SuggestedReviewers} from './SuggestedReviewers';
import {extractTokens} from './Tokens';
import {getOnClickToken} from './utils';
import {randomId} from 'shared/utils';

export function CommitInfoTextField({
  field,
  autoFocus,
  editedMessage,
  setEditedCommitMessage,
}: {
  field: FieldConfig & {type: 'field'};
  autoFocus: boolean;
  editedMessage: string;
  setEditedCommitMessage: (fieldValue: string) => unknown;
}) {
  const {key, maxTokens, typeaheadKind} = field;
  const fieldKey = key.toLowerCase().replace(/\s/g, '-');
  const isReviewers = fieldKey === 'reviewers';

  const [tokens] = extractTokens(editedMessage);

  return (
    <Typeahead
      tokenString={editedMessage}
      setTokenString={setEditedCommitMessage}
      autoFocus={autoFocus}
      maxTokens={maxTokens}
      fetchTokens={fetchNewSuggestions.bind(undefined, typeaheadKind)}
      onSaveNewToken={
        isReviewers
          ? value => {
              recentReviewers.useReviewer(value);
            }
          : undefined
      }
      data-testid={`commit-info-${fieldKey}-field`}
      onClickToken={getOnClickToken(field)}
      renderExtra={
        !isReviewers
          ? undefined
          : saveNewValue => (
              <SuggestedReviewers existingReviewers={tokens} addReviewer={saveNewValue} />
            )
      }
    />
  );
}

async function fetchNewSuggestions(
  kind: TypeaheadKind,
  text: string,
): Promise<{values: Array<TypeaheadResult>; fetchStartTimestamp: number}> {
  const now = Date.now();
  if (text.trim().length < 2) {
    // no need to do a fetch on zero- or one-char input...
    // it's slow and doesn't give good suggestions anyway
    return {values: [], fetchStartTimestamp: now};
  }
  const id = randomId();
  serverApi.postMessage({type: 'typeahead', kind, id, query: text});
  const values = await serverApi.nextMessageMatching(
    'typeaheadResult',
    message => message.id === id,
  );
  return {values: values.result, fetchStartTimestamp: now};
}
