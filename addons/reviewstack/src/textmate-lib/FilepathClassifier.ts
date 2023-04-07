/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Grammar, LanguageConfiguration} from './types';

import splitPath from './splitPath';

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

export default class FilepathClassifier {
  private index: LanguageIndex;

  constructor(
    private grammars: {[scopeName: string]: Grammar},
    languages: {[language: string]: LanguageConfiguration},
  ) {
    this.index = createIndex(languages);
  }

  findScopeNameForPath(path: string): string | null {
    const [, filename] = splitPath(path);
    const language = this.findTextMateLanguage(filename);
    return language != null ? this.findScopeNameForLanguage(language) : null;
  }

  findScopeNameForLanguage(language: string): string | null {
    for (const [scopeName, grammar] of Object.entries(this.grammars)) {
      if (grammar.language === language) {
        return scopeName;
      }
    }
    return null;
  }

  /**
   * This function is useful for mapping the tag used in a fenced code block
   * to a scope name. For example, while the language id for JavaScript is
   * `javascript` according to the LSP spec, users frequently use the alias
   * `js` when creating fenced code blocks, so we would like to be able to map
   * both to the scope name `source.js`.
   *
   * Note that the TextMate grammar for Markdown hardcodes these aliases,
   * which is useful when displaying Markdown source in an editor:
   *
   * https://github.com/microsoft/vscode/blob/ea0e3e0d1fab/extensions/markdown-basics/syntaxes/markdown.tmLanguage.json#L960
   *
   * But when rendering Markdown as HTML, clients often have to provide their
   * own syntax highlighting logic, which has to do its own mapping of the tag
   * for the fenced code block. For example, here is where highlight.js declares
   * its aliases for Javascript [sic]:
   *
   * https://github.com/highlightjs/highlight.js/blob/91e1898df92a/src/languages/javascript.js#L454
   */
  findScopeNameForAlias(alias: string): string | null {
    const language = this.index.aliases.get(alias) ?? alias;
    return this.findScopeNameForLanguage(language);
  }

  /**
   * Given a filename like `index.js` or `BUCK`, returns the language id of the
   * TextMate grammar that should be used to highlight it. This function does
   * *not* depend on Monaco, so it can be used in other contexts.
   */
  findTextMateLanguage(filename: string): string | null {
    const language = this.index.filenames.get(filename);
    if (language != null) {
      return language;
    }

    for (const [extension, language] of this.index.extensions.entries()) {
      if (filename.endsWith(extension)) {
        return language;
      }
    }

    return null;
  }
}

function createIndex(languages: {[language: string]: LanguageConfiguration}): LanguageIndex {
  const filenames = new Map();
  const extensions = new Map();
  const aliases = new Map();
  const supportedLanguages = new Set<string>();

  for (const [language, configuration] of Object.entries(languages)) {
    supportedLanguages.add(language);
    configuration.aliases?.forEach((alias: string) => {
      if (alias.toLowerCase() !== language) {
        supportedLanguages.add(alias);
        aliases.set(alias, language);
      }
    });
    const languageFilenames = configuration.filenames ?? [];
    languageFilenames.forEach((filename: string) => filenames.set(filename, language));

    const languageExtensions = configuration.extensions ?? [];
    languageExtensions.forEach((extension: string) => extensions.set(extension, language));
  }

  return {filenames, extensions, supportedLanguages, aliases};
}
