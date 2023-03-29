/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {IGrammar} from 'vscode-textmate';

import {INITIAL} from 'vscode-textmate';

// The following values come from the MetadataConsts enum in vscode-textmate.
// Although they are decalred in the main.d.ts file, our TypeScript/Webpack
// setup does not appear to be able to inline them properly.
const FOREGROUND_MASK = 8372224;
const FOREGROUND_OFFSET = 14;

export type HighlightedToken = {
  /** Start index within a line, inclusive. */
  start: number;

  /** End index within a line, exclusive. */
  end: number;

  /** Index into a color map. */
  color: number;
};

export default function tokenizeFileContents(
  fileContents: string,
  {
    grammar,
  }: {
    grammar: IGrammar;
  },
): Array<Array<HighlightedToken>> {
  let ruleStack = INITIAL;
  // As fileContents could be quite large, we are assuming that, even though
  // split() generates a potentially large array, because it is one native
  // call, it is likely to be more efficient than us doing our own bookkeeping
  // to slice off one substring at a time (though that would avoid the array
  // allocation).
  return fileContents.split('\n').map((line: string) => {
    // Line-processing logic taken from:
    // https://github.com/microsoft/vscode-textmate/blob/cc8ae321cfb47940470bd82c87a8ac61366fbd80/src/tests/themedTokenizer.ts#L20-L41
    const result = grammar.tokenizeLine2(line, ruleStack);

    // eslint-disable-next-line no-bitwise
    const tokensLength = result.tokens.length >> 1;
    const singleLine = [];
    for (let j = 0; j < tokensLength; j++) {
      const startIndex = result.tokens[2 * j];
      const nextStartIndex = j + 1 < tokensLength ? result.tokens[2 * j + 2] : line.length;
      const tokenText = line.substring(startIndex, nextStartIndex);
      if (tokenText === '') {
        continue;
      }

      const metaData = result.tokens[2 * j + 1];

      // Get foreground index from metaData so that we can index into TokensCSS
      // (a map from className to styles). Note this code comes from:
      // https://github.com/microsoft/vscode-textmate/blob/cc8ae321cfb47940470bd82c87a8ac61366fbd80/src/grammar.ts#L1032-L1034
      // We have to inline it here because StackElementMetadata does not appear
      // to be exported as part of the vscode-textmate npm module.
      // eslint-disable-next-line no-bitwise
      const foregroundIdx = (metaData & FOREGROUND_MASK) >>> FOREGROUND_OFFSET;

      singleLine.push({
        start: startIndex,
        end: nextStartIndex,
        color: foregroundIdx,
      });
    }
    ruleStack = result.ruleStack;
    return singleLine;
  });
}
