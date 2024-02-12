/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {languages} from 'monaco-editor';

import yaml from 'js-yaml';
import jsonc from 'jsonc-parser';

/* eslint-disable no-console */

export type GrammarSource =
  | {type: 'json'; definition: Record<string, unknown>}
  | {type: 'plist'; definition: string};

export type GrammarContribution = {
  language?: string | null;
  scopeName: string;
  path: string;
  tokenTypes?: Record<string, string>;
  embeddedLanguages?: Record<string, string>;
  injectTo?: string[];
};

type Override<T1, T2> = Omit<T1, keyof T2> & T2;

/**
 * `languages.ILanguageExtensionPoint` defines `configuration` as a URI, but we
 * just parse the manifest as JSON and just leave `configuration` as a raw
 * string rather than a more structured type.
 */
export type LanguageExtensionPoint =
  | Override<languages.ILanguageExtensionPoint, {configuration: string}>
  | Omit<languages.ILanguageExtensionPoint, 'configuration'>;

/**
 * This is an entry in `contributes.languages` from the extension's
 * `package.json` where the `configuration` property has been replaced by the
 * path to the `xxx-configuration.json` file with the parsed contents of that
 * file.
 */
export type NormalizedLanguageExtensionPoint =
  | Override<languages.ILanguageExtensionPoint, {configuration: languages.LanguageConfiguration}>
  | Omit<languages.ILanguageExtensionPoint, 'configuration'>;

/**
 * Parsed package.json for a VS Code extension.
 */
export type ExtensionManifest = {
  contributes: {
    languages?: LanguageExtensionPoint[];
    grammars?: GrammarContribution[];
  };
};

/**
 * Abstraction for fetching file contents/data for a VS Code language extension.
 *
 * Note this is an abstract class: subclasses must implement `getContents()` and
 * `toString()`.
 */
export default abstract class AbstractLanguageExtension {
  /**
   * @param pathRelativeToExtensionRoot relative path to the root of the
   *   extension
   * @return the contents of the corresponding file as a string
   */
  abstract getContents(pathRelativeToExtensionRoot: string): Promise<string>;

  /**
   * @return a string that identifies the source of this extension to provide
   *   appropriate context when reporting errors
   */
  abstract toString(): string;

  /** @return the parsed package.json for the extension as an Object. */
  getManifest(): Promise<ExtensionManifest> {
    return this.getContents('package.json')
      .then(text => JSON.parse(text))
      .then(manifest => this.applyCustomizationsToManifest(manifest))
      .catch(e => {
        console.error(`ERROR: failed to fetch package.json from ${this}`, e);
        throw e;
      });
  }

  /**
   * If necessary, rewrite the manifest (i.e., the parsed version of the
   * `package.json`) before making it available to callers. Designed to be
   * overridden: defaults to returning `originalManifest`.
   * @param originalManifest as parsed from the package.json
   * @return a valid manifest
   */
  applyCustomizationsToManifest(originalManifest: ExtensionManifest): ExtensionManifest {
    return originalManifest;
  }

  /**
   * @param configPath relative path within the extension to the config file
   * @return the parsed config file as an Object
   */
  getLanguageConfiguration(configPath: string): Promise<languages.LanguageConfiguration> {
    return this.getContents(configPath)
      .then(text => jsonc.parse(text))
      .catch(e => {
        console.error(`ERROR: failed to fetch ${configPath} from ${this}`, e);
        throw e;
      });
  }

  /**
   * @param grammarPath relative path within the extension to the grammar file
   */
  async getGrammar(candidateGrammarPath: string): Promise<GrammarSource> {
    let textmate;
    const grammarPath = candidateGrammarPath;
    try {
      textmate = await this.getContents(grammarPath);
    } catch (e) {
      // Before giving up hope, check for this one weird edge case...
      if (grammarPath.endsWith('.json')) {
        // On occasion (e.g., JustusAdam/language-haskell,
        // eirikpre/VSCode-SystemVerilog), an extension has been known to define
        // the grammar in YAML in the source tree (presumably so they can do
        // sane things, like include comments), but when publishing to the VS
        // Code Marketplace, they generate JSON from it. Because we can only
        // read what is checked into source control from here, we must fetch the
        // YAML version and fall through to the YAML-to-JSON conversion logic
        // below.
        const yamlExtensions = ['.yaml', '.YAML-tmLanguage'];
        for (const extension of yamlExtensions) {
          try {
            // eslint-disable-next-line no-await-in-loop
            return await this.getGrammarWithAlternateExtension(grammarPath, extension);
          } catch {
            // try the next extension
          }
        }
        throw new Error(
          `could not fetch ${grammarPath} despite trying it with alternate extensions: ${yamlExtensions}`,
        );
      } else {
        console.error(`ERROR: failed to fetch ${grammarPath} from ${this}`, e);
        throw e;
      }
    }

    // Grammar could be in plist format. Note that
    // extensions/javascript/syntaxes/Regular Expressions (JavaScript).tmLanguage
    // falls into this category.
    if (grammarPath.endsWith('.json')) {
      let definition;
      try {
        definition = JSON.parse(textmate);
      } catch (e) {
        console.error(`Error parsing contents of ${grammarPath} as JSON from ${this}`);
        throw e;
      }

      return {
        type: 'json',
        definition,
      };
    } else if (grammarPath.endsWith('.yaml') || grammarPath.endsWith('.YAML-tmLanguage')) {
      // Unfortunately, it is not a guarantee that someone who decided to
      // define their TextMate grammar in YAML is using a `.yaml` file
      // extension, e.g.:
      // https://github.com/go2sh/tcl-language-support/blob/7de6351b76a501a89c79c3da5a5cb7bc65cd0b95/syntaxes/tcl.YAML-tmLanguage
      //
      // It turns out that PLIST files appear to successfully parse as YAML, so
      // we end up hardcoding the various file extensions that we expect to
      // be YAML in practice, and only use yaml.load() in those cases.
      try {
        // Note that yaml.load() runs in "safe mode" by default as of YAML 4:
        // https://github.com/nodeca/js-yaml/blob/master/CHANGELOG.md#400---2021-01-03.
        // Unsurprisingly, "unsafe" mode has been subject to exploits in the
        // past: https://github.com/nodeca/js-yaml/pull/480.
        const definition = yaml.load(textmate) as Record<string, unknown>;
        return {
          type: 'json',
          definition,
        };
      } catch (e) {
        console.error(`Error parsing contents of ${grammarPath} as YAML from ${this}`);
        throw e;
      }
    } else {
      return {
        type: 'plist',
        definition: textmate,
      };
    }
  }

  async getGrammarWithAlternateExtension(
    grammarPath: string,
    extension: string,
  ): Promise<GrammarSource> {
    const doctoredPath = grammarPath.replace(/\.json$/, extension);
    let textmate;
    try {
      textmate = await this.getContents(doctoredPath);
    } catch (e) {
      throw new Error(`could not fetch ${doctoredPath} after trying to rewrite it due to ${e}`);
    }

    const definition = yaml.load(textmate) as Record<string, unknown>;
    return {
      type: 'json',
      definition,
    };
  }
}
