/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FieldConfig} from './types';

import {extractTokens} from 'isl-components/Tokens';
import {Typeahead} from 'isl-components/Typeahead';
import {recentReviewers, SuggestedReviewers} from './SuggestedReviewers';
import {convertFieldNameToKey, fetchNewSuggestions, getOnClickToken} from './utils';

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
  const fieldKey = convertFieldNameToKey(key);
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
