/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TypeaheadKind, TypeaheadResult} from './types';

import serverApi from '../ClientToServerAPI';
import {Subtle} from '../Subtle';
import {recentReviewers, SuggestedReviewers} from './SuggestedReviewers';
import {extractTokens, TokensList, tokensToString} from './Tokens';
import {getInnerTextareaForVSCodeTextArea} from './utils';
import {VSCodeTextField} from '@vscode/webview-ui-toolkit/react';
import {useRef, useEffect, useState} from 'react';
import {Icon} from 'shared/Icon';
import {randomId} from 'shared/utils';

export function CommitInfoTextField({
  name,
  autoFocus,
  editedMessage,
  setEditedCommitMessage,
  typeaheadKind,
  maxTokens,
}: {
  name: string;
  autoFocus: boolean;
  editedMessage: string;
  setEditedCommitMessage: (fieldValue: string) => unknown;
  typeaheadKind: TypeaheadKind;
  maxTokens?: number;
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
      // don't show typeahead suggestions that are already entered
      const newValues = values.filter(v => !tokens.includes(v.value));

      setTypeaheadSuggestions(last =>
        last?.type === 'success' && last.timestamp > fetchStartTimestamp
          ? // this result is older than the one we've already set: ignore it
            last
          : {type: 'success', values: newValues, timestamp: fetchStartTimestamp},
      );
    });
  };

  const fieldKey = name.toLowerCase().replace(/\s/g, '-');

  const isReviewers = fieldKey === 'reviewers';

  const saveNewValue = (value: string | undefined) => {
    if (value && !tokens.includes(value)) {
      setEditedCommitMessage(
        tokensToString(
          tokens,
          // add comma to end the token
          value + ',',
        ),
      );
      // clear out typeahead
      setTypeaheadSuggestions({type: 'success', values: [], timestamp: Date.now()});

      // save as recent reviewer, if applicable
      if (isReviewers) {
        recentReviewers.useReviewer(value);
      }
    }
  };

  return (
    <>
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
        <TokensList
          tokens={tokens}
          onClickX={(token: string) => {
            setEditedCommitMessage(
              tokensToString(
                tokens.filter(t => t !== token),
                // keep anything already typed in
                (ref.current as HTMLInputElement | null)?.value ?? '',
              ),
            );
          }}
        />
        {tokens.length >= (maxTokens ?? Infinity) ? null : (
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
                        'suggestion' +
                        (index === selectedSuggestionIndex ? ' selected-suggestion' : '')
                      }
                      onMouseDown={() => {
                        saveNewValue(suggestion.value);
                      }}>
                      {suggestion.image && <ImageWithFallback src={suggestion.image} />}
                      <span className="suggestion-label">
                        <span>{suggestion.label}</span>
                        {suggestion.label !== suggestion.value && (
                          <Subtle>{suggestion.value}</Subtle>
                        )}
                      </span>
                    </span>
                  ))
                )}
              </div>
            ) : null}
          </div>
        )}
      </div>
      {isReviewers && <SuggestedReviewers existingReviewers={tokens} addReviewer={saveNewValue} />}
    </>
  );
}

const TRANSPARENT_1PX_GIF =
  'data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==';
function ImageWithFallback({
  src,
  ...rest
}: {src: string} & React.DetailedHTMLProps<
  React.ImgHTMLAttributes<HTMLImageElement>,
  HTMLImageElement
>) {
  return (
    <img
      src={src}
      onError={e => {
        // Images that fail to load would show a broken image icon.
        // Instead, on error we can replace the image src with a transparent 1x1 gif to hide it
        // and use our CSS fallback.
        if (e.target) {
          (e.target as HTMLImageElement).src = TRANSPARENT_1PX_GIF;
        }
      }}
      {...rest}
    />
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
