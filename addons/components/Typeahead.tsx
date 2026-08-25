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

/**
 * Fetching suggestions is expensive (on some platforms each fetch is a subprocess),
 * so don't fetch until typing pauses for this long.
 */
const DEFAULT_DEBOUNCE_INTERVAL_MS = 300;

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
  /** How long to wait after the last keystroke before fetching. Defaults to 300ms. */
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

  // Call sites typically pass an inline arrow or a `.bind()` for `fetchTokens`, so it is a fresh
  // function on every render and the debouncer must not be rebuilt from its identity — a debouncer
  // that is replaced between keystrokes never coalesces them.
  const fetchTokensRef = useRef(fetchTokens);
  useEffect(() => {
    fetchTokensRef.current = fetchTokens;
  }, [fetchTokens]);

  /**
   * The raw text last typed into the field, before tokenisation — not always what the input
   * renders, since `extractTokens` may since have split part of it into a token. Written
   * imperatively rather than read off a render, because a fetch resolving has to see the field as
   * it was at that instant. Only keystrokes and accepts write it, so where it does diverge it can
   * only leave the guard below more permissive, never stricter than reading the rendered value.
   */
  const currentQueryRef = useRef(remaining);

  const fetchTokenHandler = useCallback((value: string, previousTokens: Array<string>) => {
    fetchTokensRef.current(value).then(({values, fetchStartTimestamp}) => {
      // Results are only offered for a query the field still extends. Anything else describes text
      // that has since been deleted or replaced, and there is nothing left to unseat it: the next
      // fetch is a typing pause away, so the dropdown would sit there contradicting the input and
      // Enter would commit out of it. The keystroke path cannot cover this — the list arrives after
      // the last keystroke, so no keystroke is left to evaluate it.
      if (!currentQueryRef.current.startsWith(value)) {
        return;
      }

      // don't show typeahead suggestions that are already entered
      const newValues = values.filter(v => !previousTokens.includes(v.value));

      setTypeaheadSuggestions(last =>
        last?.type === 'success' && last.timestamp > fetchStartTimestamp
          ? // this result is older than the one we've already set: ignore it
            last
          : {type: 'success', values: newValues, timestamp: fetchStartTimestamp, prefix: value},
      );
    });
  }, []);

  const debouncedFetchTokenHandler = useMemo(() => {
    return debounce(fetchTokenHandler, debounceInterval ?? DEFAULT_DEBOUNCE_INTERVAL_MS);
  }, [debounceInterval, fetchTokenHandler]);

  useEffect(() => () => debouncedFetchTokenHandler.dispose(), [debouncedFetchTokenHandler]);

  const onInput = (event: {target: EventTarget | null}) => {
    // The optional chain can yield undefined, and every use below is a string method.
    const newValue = (event?.target as HTMLInputElement)?.value ?? '';
    setTokenString(tokensToString(tokens, newValue));
    currentQueryRef.current = newValue;

    // Deliberately weak: only a query that replaces the one the list was fetched for invalidates
    // it. Extending it (`ali` -> `alic`) may have stopped matching, and shortening it (`alicee` ->
    // `alice`) leaves the list merely incomplete — in both cases the refetch a typing pause away
    // settles it, while blanking the list would flicker the dropdown shut mid-word and take the
    // arrow keys with it. Enter is gated separately on the list actually matching what was typed,
    // which is what keeps a list held over here from being committed out of.
    const landed = typeaheadSuggestions?.type === 'success' ? typeaheadSuggestions : undefined;
    const stillWorthShowing =
      landed != null &&
      landed.values.length > 0 &&
      (newValue.startsWith(landed.prefix) ||
        (newValue !== '' && landed.prefix.startsWith(newValue)));
    if (!stillWorthShowing) {
      setTypeaheadSuggestions({type: 'loading'});
    }

    debouncedFetchTokenHandler(newValue, tokens);
  };

  const saveNewValue = (value: string | undefined) => {
    if (value && !tokens.includes(value)) {
      setTokenString(tokensToString([...tokens, value], ''));
      currentQueryRef.current = '';
      // The keystrokes leading here scheduled a fetch whose answer nobody now wants, and on some
      // platforms a fetch is a subprocess.
      debouncedFetchTokenHandler.reset();
      // clear out typeahead
      setTypeaheadSuggestions({type: 'success', values: [], timestamp: Date.now(), prefix: ''});

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

          const landed =
            typeaheadSuggestions?.type === 'success' ? typeaheadSuggestions : undefined;
          if (landed == null) {
            return;
          }
          const values = landed.values;

          if (event.key === 'ArrowDown') {
            setSelectedIndex(last => Math.min(last + 1, values.length - 1));
            event.preventDefault();
          } else if (event.key === 'ArrowUp') {
            // allow -1, so you can up arrow "above" the top, to make it highlight nothing
            setSelectedIndex(last => Math.max(last - 1, -1));
            event.preventDefault();
          } else if (event.key === 'Enter') {
            // `onInput` deliberately keeps the list up while the query only grows or shrinks, so it
            // can be on screen describing text the user has already typed past. Arrowing through a
            // list like that is harmless; committing out of it is not — Enter would write `alan`
            // after the user typed `alic`, and `saveNewValue` would discard the `ic` along with it.
            // Wait for the refetch, which is at most one debounce interval away.
            if (landed.prefix === currentQueryRef.current) {
              saveNewValue(values[selectedSuggestionIndex].value);
            }
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
            <TextField {...rest} ref={ref} value={remaining} onInput={onInput} />
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
  | {
      type: 'success';
      values: Array<TypeaheadResult>;
      timestamp: number;
      /** Query these values were fetched for, so a query that stops extending it can drop them. */
      prefix: string;
    }
  | undefined;
