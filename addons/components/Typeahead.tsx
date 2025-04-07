/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TypeaheadResult} from './Types';
import type {ReactProps} from './utils';

import {useCallback, useEffect, useMemo, useRef, useState} from 'react';
import {debounce} from 'shared/debounce';
import {Icon} from './Icon';
import {Subtle} from './Subtle';
import {TextField} from './TextField';
import {extractTokens, TokensList, tokensToString} from './Tokens';

export function Typeahead({
  tokenString,
  setTokenString,
  fetchTokens,
  onSaveNewToken,
  onClickToken,
  renderExtra,
  maxTokens,
  autoFocus,
  debounceInterval,
  ...rest
}: {
  tokenString: string;
  setTokenString: (newValue: string) => void;
  fetchTokens: (
    prefix: string,
  ) => Promise<{values: Array<TypeaheadResult>; fetchStartTimestamp: number}>;
  onSaveNewToken?: (newValue: string) => void;
  onClickToken?: (token: string) => void;
  /** Render more content below typeahead, useful for buttons that can add new tokens */
  renderExtra?: (saveNewValue: (value: string) => void) => React.ReactNode;
  maxTokens?: number;
  autoFocus: boolean;
  debounceInterval?: number;
} & ReactProps<HTMLInputElement>) {
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (ref.current && autoFocus) {
      ref.current?.focus();
    }
  }, [autoFocus, ref]);

  const [tokens, remaining] = extractTokens(tokenString);

  const [typeaheadSuggestions, setTypeaheadSuggestions] = useState<TypeaheadSuggestions>(undefined);

  const [selectedSuggestionIndex, setSelectedIndex] = useState(0);

  const fetchTokenHandler = useCallback(
    (value: string, previousTokens: Array<string>) => {
      fetchTokens(value).then(({values, fetchStartTimestamp}) => {
        // don't show typeahead suggestions that are already entered
        const newValues = values.filter(v => !previousTokens.includes(v.value));

        setTypeaheadSuggestions(last =>
          last?.type === 'success' && last.timestamp > fetchStartTimestamp
            ? // this result is older than the one we've already set: ignore it
              last
            : {type: 'success', values: newValues, timestamp: fetchStartTimestamp},
        );
      });
    },
    [fetchTokens],
  );

  const debouncedFetchTokenHandler = useMemo(() => {
    return debounce(fetchTokenHandler, debounceInterval ?? 0);
  }, [debounceInterval, fetchTokenHandler]);

  const onInput = (event: {target: EventTarget | null}) => {
    const newValue = (event?.target as HTMLInputElement)?.value;
    setTokenString(tokensToString(tokens, newValue));

    if (typeaheadSuggestions?.type !== 'success' || typeaheadSuggestions.values.length === 0) {
      setTypeaheadSuggestions({type: 'loading'});
    }

    debounceInterval
      ? debouncedFetchTokenHandler(newValue, tokens)
      : fetchTokenHandler(newValue, tokens);
  };

  const saveNewValue = (value: string | undefined) => {
    if (value && !tokens.includes(value)) {
      setTokenString(tokensToString([...tokens, value], ''));
      // clear out typeahead
      setTypeaheadSuggestions({type: 'success', values: [], timestamp: Date.now()});

      onSaveNewToken?.(value);
    }
  };

  return (
    <>
      <div
        className="commit-info-tokenized-field"
        onKeyDown={event => {
          if (event.key === 'Backspace' && ref.current?.value.length === 0) {
            // pop one token off
            setTokenString(tokensToString(tokens.slice(0, -1), ''));
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
            event.preventDefault();
          }
        }}>
        <TokensList
          tokens={tokens}
          onClickToken={onClickToken}
          onClickX={(token: string) => {
            setTokenString(
              tokensToString(
                tokens.filter(t => t !== token),
                // keep anything already typed in
                ref.current?.value ?? '',
              ),
            );
          }}
        />
        {tokens.length >= (maxTokens ?? Infinity) ? null : (
          <div className="commit-info-field-with-typeahead">
            <TextField ref={ref} value={remaining} onInput={onInput} {...rest} />
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
                        {(suggestion.detail || suggestion.label !== suggestion.value) && (
                          <Subtle>{suggestion.detail ?? suggestion.value}</Subtle>
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
      {renderExtra?.(saveNewValue)}
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
