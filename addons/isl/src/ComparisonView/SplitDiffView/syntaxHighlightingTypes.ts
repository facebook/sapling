/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ThemeColor} from '../../theme';
import type {ParsedDiff} from 'shared/patch/parse';
import type {HighlightedToken} from 'shared/textmate-lib/tokenize';

export type TokenizedHunk = Array<Array<HighlightedToken>>;
export type TokenizedDiffHunk = [before: TokenizedHunk, after: TokenizedHunk];
export type TokenizedDiffHunks = Array<TokenizedDiffHunk>;

export type SyntaxWorkerRequest =
  | {
      type: 'setBaseUri';
      base: string;
    }
  | {
      type: 'cancel';
      idToCancel: number;
    }
  | {
      type: 'tokenizeContents';
      theme: ThemeColor;
      path: string;
      content: Array<string>;
    }
  | {
      type: 'tokenizeHunks';
      theme: ThemeColor;
      path: string;
      hunks: ParsedDiff['hunks'];
    };

export type SyntaxWorkerResponse =
  | {
      type: 'cssColorMap';
      colorMap: string[];
    }
  | {
      type: 'tokenizeContents';
      result: TokenizedHunk | undefined;
    }
  | {
      type: 'tokenizeHunks';
      result: TokenizedDiffHunks | undefined;
    };
