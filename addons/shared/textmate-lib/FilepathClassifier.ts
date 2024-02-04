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
    private languages: {[language: string]: LanguageConfiguration},
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

  /**
   * Makes a best-effort to map the specified language id (like `fsharp`) to a
   * name that is more familiar to the user (like `F#`). Also supports alises
   * so that both `py` and `python` are mapped to `Python`.
   */
  getDisplayNameForLanguageId(languageIdOrAlias: string): string {
    const scopeName =
      this.findScopeNameForLanguage(languageIdOrAlias) ??
      this.findScopeNameForAlias(languageIdOrAlias);
    if (scopeName == null) {
      return languageIdOrAlias;
    }

    return this.findDisplayNameForScopeName(scopeName) ?? languageIdOrAlias;
  }

  /**
   * Try to return a human-readable name for the specified scope name.
   * Unfortunately, VS Code does not currently expose the language name
   * directly: https://github.com/microsoft/vscode/issues/109919. As a
   * workaround, we make our best guess from the available aliases associated
   * with the scope name.
   */
  findDisplayNameForScopeName(scopeName: string): string | null {
    const {language} = this.grammars[scopeName] ?? {};
    if (language != null) {
      const aliases = this.languages[language].aliases ?? [];
      // As a braindead heuristic, we pick the first alias that starts with a
      // capital letter.
      for (const alias of aliases) {
        const firstChar = alias.charAt(0);
        if (firstChar.toUpperCase() === firstChar) {
          return alias;
        }
      }

      // If none of the aliases start with a capital letter, pick the first.
      return aliases[0] ?? null;
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
