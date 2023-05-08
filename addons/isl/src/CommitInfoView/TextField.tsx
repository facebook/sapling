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

  const onInput = (event: {target: EventTarget | null}) => {
    const newValue = (event?.target as HTMLInputElement)?.value;
    setEditedCommitMessage(tokensToString(tokens, newValue));
    setTypeaheadSuggestions({type: 'loading'});
    fetchNewSuggestions(typeaheadKind, newValue).then(values =>
      setTypeaheadSuggestions({type: 'success', values}),
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
      <div className="commit-info-field-with-typeahead">
        <VSCodeTextField
          ref={ref}
          value={remaining}
          data-testid={`commit-info-${fieldKey}-field`}
          onInput={onInput}
        />
        {typeaheadSuggestions?.type === 'loading' ||
        (typeaheadSuggestions?.values?.length ?? 0) > 0 ? (
          <div className="typeahead-suggestions">
            {typeaheadSuggestions?.type === 'loading' ? (
              <Icon icon="loading" />
            ) : (
              typeaheadSuggestions?.values.map(suggestion => (
                <span key={suggestion.value} className="suggestion">
                  {suggestion.image && <img src={suggestion.image} alt={suggestion.label} />}
                  <span className="suggestion-label">
                    <span>{suggestion.label}</span>
                    <Subtle>{suggestion.value}</Subtle>
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
  | {type: 'success'; values: Array<TypeaheadResult>}
  | undefined;

// eslint-disable-next-line require-await
async function fetchNewSuggestions(
  kind: TypeaheadKind,
  text: string,
): Promise<Array<TypeaheadResult>> {
  const id = randomId();
  serverApi.postMessage({type: 'typeahead', kind, id, query: text});
  const values = await serverApi.nextMessageMatching(
    'typeaheadResult',
    message => message.id === id,
  );
  return values.result;
}
