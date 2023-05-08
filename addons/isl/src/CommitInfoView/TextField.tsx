/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TypeaheadKind, TypeaheadResult} from './types';

import serverApi from '../ClientToServerAPI';
import {Subtle} from '../Subtle';
import {getInnerTextareaForVSCodeTextArea} from './utils';
import {VSCodeButton, VSCodeTextField} from '@vscode/webview-ui-toolkit/react';
import {useRef, useEffect, useState} from 'react';
import {Icon} from 'shared/Icon';
import {randomId} from 'shared/utils';

/** Extract comma-separated tokens into an array, plus any remaining non-tokenized text */
function extractTokens(raw: string): [Array<string>, string] {
  const tokens = raw.split(',');
  const remaining = tokens.length === 0 ? raw : tokens.pop();
  return [tokens.map(token => token.trim()), remaining ?? ''];
}

/** Combine tokens back into a string to be stored in the commit message */
function tokensToString(tokens: Array<string>, remaining: string): string {
  return tokens.length === 0 ? remaining : tokens.join(',') + ',' + remaining;
}

export function CommitInfoTextField({
  name,
  autoFocus,
  editedMessage,
  setEditedCommitMessage,
  typeaheadKind,
}: {
  name: string;
  autoFocus: boolean;
  editedMessage: string;
  setEditedCommitMessage: (fieldValue: string) => unknown;
  typeaheadKind: TypeaheadKind;
}) {
  const ref = useRef(null);
  useEffect(() => {
    if (ref.current && autoFocus) {
      const inner = getInnerTextareaForVSCodeTextArea(ref.current as HTMLElement);
      inner?.focus();
    }
  }, [autoFocus, ref]);

  const [tokens, remaining] = extractTokens(editedMessage);

  const [typeaheadSuggestions, setTypeaheadSuggestions] = useState<TypeaheadSuggestions>(undefined);

  const [selectedSuggestionIndex, setSelectedIndex] = useState(0);

  const onInput = (event: {target: EventTarget | null}) => {
    const newValue = (event?.target as HTMLInputElement)?.value;
    setEditedCommitMessage(tokensToString(tokens, newValue));
    if (typeaheadSuggestions?.type !== 'success' || typeaheadSuggestions.values.length === 0) {
      setTypeaheadSuggestions({type: 'loading'});
    }
    fetchNewSuggestions(typeaheadKind, newValue).then(({values, fetchStartTimestamp}) => {
      setTypeaheadSuggestions(last =>
        last?.type === 'success' && last.timestamp > fetchStartTimestamp
          ? // this result is older than the one we've already set: ignore it
            last
          : {type: 'success', values, timestamp: fetchStartTimestamp},
      );
    });
  };

  const fieldKey = name.toLowerCase().replace(/\s/g, '-');

  const saveNewValue = (value: string | undefined) => {
    if (value) {
      setEditedCommitMessage(
        tokensToString(
          tokens,
          // add comma to end the token
          value + ',',
        ),
      );
      // clear out typeahead
      setTypeaheadSuggestions({type: 'success', values: [], timestamp: Date.now()});
    }
  };

  return (
    <div
      className="commit-info-tokenized-field"
      onKeyDown={event => {
        if (
          event.key === 'Backspace' &&
          (ref.current as HTMLInputElement | null)?.value.length === 0
        ) {
          // pop one token off
          setEditedCommitMessage(tokensToString(tokens.slice(0, -1), ''));
          return;
        }

        const values = (typeaheadSuggestions as TypeaheadSuggestions & {type: 'success'})?.values;
        if (values == null) {
          return;
        }

        if (event.key === 'ArrowDown') {
          setSelectedIndex(last => Math.min(last + 1, values.length - 1));
          event.preventDefault();
        } else if (event.key === 'ArrowUp') {
          // allow -1, so you can up arrow "above" the top, to make it highlight nothing
          setSelectedIndex(last => Math.max(last - 1, -1));
          event.preventDefault();
        } else if (event.key === 'Enter') {
          saveNewValue(values[selectedSuggestionIndex].value);
        }
      }}>
      {tokens
        .filter(token => token != '')
        .map((token, i) => (
          <span key={i} className="token">
            {token}
            <VSCodeButton
              appearance="icon"
              onClick={() => {
                setEditedCommitMessage(
                  tokensToString(
                    tokens.filter(t => t !== token),
                    // keep anything already typed in
                    (ref.current as HTMLInputElement | null)?.value ?? '',
                  ),
                );
              }}>
              <Icon icon="x" />
            </VSCodeButton>
          </span>
        ))}
      <div className="commit-info-field-with-typeahead">
        <VSCodeTextField
          ref={ref}
          value={remaining}
          data-testid={`commit-info-${fieldKey}-field`}
          onInput={onInput}
        />
        {typeaheadSuggestions?.type === 'loading' ||
        (typeaheadSuggestions?.values?.length ?? 0) > 0 ? (
          <div className="typeahead-suggestions tooltip tooltip-bottom">
            <div className="tooltip-arrow tooltip-arrow-bottom" />
            {typeaheadSuggestions?.type === 'loading' ? (
              <Icon icon="loading" />
            ) : (
              typeaheadSuggestions?.values.map((suggestion, index) => (
                <span
                  key={suggestion.value}
                  className={
                    'suggestion' + (index === selectedSuggestionIndex ? ' selected-suggestion' : '')
                  }
                  onMouseDown={() => {
                    saveNewValue(suggestion.value);
                  }}>
                  {suggestion.image && <img src={suggestion.image} alt={suggestion.label} />}
                  <span className="suggestion-label">
                    <span>{suggestion.label}</span>
                    {suggestion.label !== suggestion.value && <Subtle>{suggestion.value}</Subtle>}
                  </span>
                </span>
              ))
            )}
          </div>
        ) : null}
      </div>
    </div>
  );
}

type TypeaheadSuggestions =
  | {
      type: 'loading';
    }
  | {type: 'success'; values: Array<TypeaheadResult>; timestamp: number}
  | undefined;

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
