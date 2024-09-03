/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from './Button';
import {Icon} from './Icon';

export function TokensList({
  tokens,
  onClickX,
  onClickToken,
}: {
  tokens: Array<string>;
  onClickX?: (token: string) => unknown;
  onClickToken?: (token: string) => unknown;
}) {
  const hasOnClick = onClickToken != null;
  return (
    <>
      {tokens
        .filter(token => token != '')
        .map((token, i) => (
          <span
            key={i}
            className={'token' + (hasOnClick ? ' clickable' : '')}
            onClick={
              hasOnClick
                ? e => {
                    onClickToken(token);
                    e.preventDefault();
                    e.stopPropagation();
                  }
                : undefined
            }>
            {token}
            {onClickX == null ? null : (
              <Button
                icon
                data-testid="token-x"
                onClick={e => {
                  onClickX?.(token);
                  e.stopPropagation();
                }}>
                <Icon icon="x" />
              </Button>
            )}
          </span>
        ))}
    </>
  );
}

function deduplicate<T>(values: Array<T>) {
  return [...new Set(values)];
}

/** Extract comma-separated tokens into an array, plus any remaining non-tokenized text */
export function extractTokens(raw: string): [Array<string>, string] {
  const tokens = raw.split(',');
  const remaining = tokens.length === 0 ? raw : tokens.pop();
  return [
    deduplicate(tokens.map(token => token.trim())).filter(token => token !== ''),
    remaining?.trimStart() ?? '',
  ];
}

/** Combine tokens back into a string to be stored in the commit message */
export function tokensToString(tokens: Array<string>, remaining: string): string {
  return tokens.length === 0 ? remaining : tokens.join(',') + ',' + remaining;
}
