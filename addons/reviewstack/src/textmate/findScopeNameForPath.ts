/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {grammars, languages} from '../generated/textmate/TextMateGrammarManifest';
import {splitPath} from '../utils';

type LanguageIndex = {
  /** File name to language id. */
  filenames: Map<string, string>;
  /** File extension to language id. */
  extensions: Map<string, string>;
  /** Language alias to language id. */
  aliases: Map<string, string>;
  /** All supported language ids. */
  supportedLanguages: Set<string>;
};

let index: LanguageIndex | null = null;

export function findScopeNameForPath(path: string): string | null {
  const [, filename] = splitPath(path);
  const language = findTextMateLanguage(filename);
  return language != null ? findScopeNameForLanguage(language) : null;
}

function findScopeNameForLanguage(language: string): string | null {
  for (const [scopeName, grammar] of Object.entries(grammars)) {
    if (grammar.language === language) {
      return scopeName;
    }
  }
  return null;
}

/**
 * Given a filename like `index.js` or `BUCK`, returns the language id of the
 * TextMate grammar that should be used to highlight it. This function does
 * *not* depend on Monaco, so it can be used in other contexts.
 */
function findTextMateLanguage(filename: string): string | null {
  if (index == null) {
    index = createIndex();
  }

  const language = index.filenames.get(filename);
  if (language != null) {
    return language;
  }

  for (const [extension, language] of index.extensions.entries()) {
    if (filename.endsWith(extension)) {
      return language;
    }
  }

  return null;
}

function createIndex(): LanguageIndex {
  const filenames = new Map();
  const extensions = new Map();
  const aliases = new Map();
  const supportedLanguages = new Set<string>();

  for (const [language, configuration] of Object.entries(languages)) {
    supportedLanguages.add(language);
    configuration.aliases?.forEach(alias => {
      if (alias.toLowerCase() !== language) {
        supportedLanguages.add(alias);
        aliases.set(alias, language);
      }
    });
    const languageFilenames = configuration.filenames ?? [];
    languageFilenames.forEach(filename => filenames.set(filename, language));

    const languageExtensions = configuration.extensions ?? [];
    languageExtensions.forEach(extension => extensions.set(extension, language));
  }

  return {filenames, extensions, supportedLanguages, aliases};
}
