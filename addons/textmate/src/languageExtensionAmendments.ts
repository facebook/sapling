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
  systemverilog: {
    extensions: ['.svi', '.v', '.vh', '.SV'],
  },
  yaml: {
    filenames: ['.prettierrc'],
  },
};

export default languageExtensionAmendments;
