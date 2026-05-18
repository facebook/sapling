/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {FieldConfig} from './types';

import {extractTokens} from 'isl-components/Tokens';
import {Typeahead} from 'isl-components/Typeahead';
import {recentReviewers, SuggestedReviewers} from './SuggestedReviewers';
import {recentTags, SuggestedTags} from './SuggestedTags';
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

  const [tokens] = extractTokens(editedMessage);

  let onSaveNewToken: ((value: string) => void) | undefined;
  let renderExtra: ((saveNewValue: (value: string) => void) => ReactNode) | undefined;

  switch (fieldKey) {
    case 'reviewers':
      onSaveNewToken = (value: string) => {
        recentReviewers.use(value);
      };
      renderExtra = (saveNewValue: (value: string) => void) => (
        <SuggestedReviewers existingReviewers={tokens} addReviewer={saveNewValue} />
      );
      break;
    case 'tags':
      onSaveNewToken = (value: string) => {
        recentTags.use(value);
      };
      renderExtra = (saveNewValue: (value: string) => void) => (
        <SuggestedTags existingTags={tokens} addTag={saveNewValue} />
      );
      break;
  }

  return (
    <Typeahead
      tokenString={editedMessage}
      setTokenString={setEditedCommitMessage}
      autoFocus={autoFocus}
      maxTokens={maxTokens}
      fetchTokens={fetchNewSuggestions.bind(undefined, typeaheadKind)}
      onSaveNewToken={onSaveNewToken}
      data-testid={`commit-info-${fieldKey}-field`}
      onClickToken={getOnClickToken(field)}
      renderExtra={renderExtra}
    />
  );
}
