/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {Icon} from 'shared/Icon';

export function TokensList({
  tokens,
  onClickX,
}: {
  tokens: Array<string>;
  onClickX?: (token: string) => unknown;
}) {
  return (
    <>
      {tokens
        .filter(token => token != '')
        .map((token, i) => (
          <span key={i} className="token">
            {token}
            {onClickX == null ? null : (
              <VSCodeButton appearance="icon" onClick={() => onClickX?.(token)}>
                <Icon icon="x" />
              </VSCodeButton>
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
  return [deduplicate(tokens.map(token => token.trim())), remaining ?? ''];
}

/** Combine tokens back into a string to be stored in the commit message */
export function tokensToString(tokens: Array<string>, remaining: string): string {
  return tokens.length === 0 ? remaining : tokens.join(',') + ',' + remaining;
}
