/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export type TextMateGrammar = {
  type: 'json' | 'plist';
  /**
   * Grammar data as a string because parseRawGrammar() in vscode-textmate
   * takes the contents as a string, even if the type is json.
   */
  grammar: string;
};

export type Grammar = {
  language?: string;
  injections: Array<string>;
  embeddedLanguages?: {[scopeName: string]: string};
  getGrammar: () => Promise<TextMateGrammar>;
};
