/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Subtle} from '../Subtle';
import {getInnerTextareaForVSCodeTextArea} from './utils';
import {VSCodeButton, VSCodeTextField} from '@vscode/webview-ui-toolkit/react';
import {useRef, useEffect, useState} from 'react';
import {Icon} from 'shared/Icon';

/** Extract comma-separated tokens into an array, plus any remaining non-tokenized text */
function extractTokens(raw: string): [Array<string>, string] {
  const tokens = raw.split(',').map(token => token.trim());
  const remaining = tokens.length === 0 ? raw : tokens.pop();
  return [tokens, remaining ?? ''];
}

/** Combine tokens back into a string to be stored in the commit message */
function tokensToString(tokens: Array<string>, remaining: string): string {
  return tokens.join(',') + ',' + remaining;
}

export function CommitInfoTextField({
  name,
  autoFocus,
  editedMessage,
  setEditedCommitMessage,
}: {
  name: string;
  autoFocus: boolean;
  editedMessage: string;
  setEditedCommitMessage: (fieldValue: string) => unknown;
}) {
  const ref = useRef(null);
  useEffect(() => {
    if (ref.current && autoFocus) {
      const inner = getInnerTextareaForVSCodeTextArea(ref.current as HTMLElement);
      inner?.focus();
    }
  }, [autoFocus, ref]);

  const [tokens, remaining] = extractTokens(editedMessage);

  const [autocompleteSuggestions, setAutocompleteSuggestions] =
    useState<AutocompleteSuggestions>(undefined);

  const onInput = (event: {target: EventTarget | null}) => {
    const newValue = (event?.target as HTMLInputElement)?.value;
    setEditedCommitMessage(tokensToString(tokens, newValue));
    setAutocompleteSuggestions({type: 'loading'});
    fetchNewSuggestions(newValue).then(values =>
      setAutocompleteSuggestions({type: 'success', values}),
    );
  };

  const fieldKey = name.toLowerCase().replace(/\s/g, '-');

  return (
    <div className="commit-info-tokenized-field">
      {tokens
        .filter(token => token != '')
        .map((token, i) => (
          <span key={i} className="token">
            {token}
            <VSCodeButton appearance="icon">
              <Icon icon="x" />
            </VSCodeButton>
          </span>
        ))}
      <div className="commit-info-field-with-autocomplete">
        <VSCodeTextField
          ref={ref}
          value={remaining}
          data-testid={`commit-info-${fieldKey}-field`}
          onInput={onInput}
        />
        {autocompleteSuggestions?.type === 'loading' ||
        (autocompleteSuggestions?.values?.length ?? 0) > 0 ? (
          <div className="autocomplete-suggestions">
            {autocompleteSuggestions?.type === 'loading' ? (
              <Icon icon="loading" />
            ) : (
              autocompleteSuggestions?.values.map(suggestion => (
                <span key={suggestion.value} className="suggestion">
                  <span>{suggestion.display}</span>
                  <Subtle>{suggestion.value}</Subtle>
                </span>
              ))
            )}
          </div>
        ) : null}
      </div>
    </div>
  );
}

type AutocompleteSuggestions =
  | {
      type: 'loading';
    }
  | {type: 'success'; values: Array<AutocompleteSuggestion>}
  | undefined;
type AutocompleteSuggestion = {
  /** The display text of the suggestion */
  display: string;
  /**
   * The literal value of the suggestion,
   * shown de-emphasized next to the display name
   * and placed literally as text into the commit message
   */
  value: string;
};

// eslint-disable-next-line require-await
async function fetchNewSuggestions(_text: string): Promise<Array<AutocompleteSuggestion>> {
  return [];
}
