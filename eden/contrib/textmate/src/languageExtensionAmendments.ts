/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {languages} from 'monaco-editor';

export type LanguageExtensionAmendment = Omit<
  languages.ILanguageExtensionPoint,
  'id' | 'configuration'
>;

/**
 * There are all sorts of additional file names/extensions that benefit from
 * syntax highlighting that are not included in the definition within the
 * grammar's corresponding VS Code extension. We must amend the
 * ILanguageExtensionPoint for each language in the map to handle these cases
 * that are not covered by default.
 */
const languageExtensionAmendments: {[language: string]: LanguageExtensionAmendment} = {
  cpp: {
    aliases: ['c++'],
    extensions: ['.cu', '.cuh'],
  },
  ini: {
    filenames: ['.buckconfig', '.flowconfig'],
    extensions: ['.bcfg', '.net'],
  },
  // If we see a fenced code block tagged as `php`,
  // we should syntax highlight it as Hack.
  hack: {
    aliases: ['php'],
  },
  rust: {
    // Note the TextMate grammar recognizes `rs` as an alias for Rust:
    //
    // https://github.com/microsoft/vscode/blob/ea0e3e0d1fab/extensions/markdown-basics/syntaxes/markdown.tmLanguage.json#L1389
    //
    // though the VS Code extension does not list it as such:
    //
    // https://github.com/microsoft/vscode/blob/ea0e3e0d1fab/extensions/rust/package.json#L21-L24
    //
    // But we include it so that `FilepathClassifier.findScopeNameForAlias('rs')`
    // returns `"source.rust"`.
    aliases: ['rs'],
  },
  systemverilog: {
    extensions: ['.svi', '.v', '.vh', '.SV'],
  },
  yaml: {
    filenames: ['.prettierrc'],
  },
};

export default languageExtensionAmendments;
