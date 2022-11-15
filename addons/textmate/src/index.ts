/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  GrammarContribution,
  GrammarSource,
  NormalizedLanguageExtensionPoint,
} from './AbstractLanguageExtension';
import type {LanguageExtensionAmendment} from './languageExtensionAmendments';

import extensions from './extensions';
import languageExtensionAmendments from './languageExtensionAmendments';
import assert from 'assert';
import {promises as fs} from 'fs';
import pathMod from 'path';
import prettier from 'prettier';

type GrammarResponse = {
  scopeName: string;
  language?: string | null;
} & GrammarSource;

type GrammarLoadingInfo = Omit<GrammarContribution, 'path'> & {
  type: 'json' | 'plist';

  /** Name of JavaScript module for loader. */
  jsModule: string;

  /** List of scope names. */
  injections: string[];
};

type ScopeName = string;

/**
 * Note that some VS Code extensions define the same grammar twice, but with
 * different values of "language". We don't support multiple declarations for a
 * scope name yet, so for now, ignore the duplicates.
 */
const IGNORED_LANGUAGES = new Set([
  'dockercompose',
  'kotlinscript',
  'properties',
  'tfvars',
  'verilog',
]);

/**
 * In the rare cases where multiple extensions try to define the grammar for a
 * scopeName, add an entry to this map to indicate which one to use.
 */
const EXTENSION_FOR_SCOPE_NAME = new Map();

type IndexedLanguageExtensionPoint = {
  index: number;
  config: NormalizedLanguageExtensionPoint;
};

async function main() {
  const [manifestPath, grammarsDir] = process.argv.slice(2, 4);
  if (manifestPath == null) {
    throw Error('must specify a file for the TextMate grammar manifest');
  }
  if (grammarsDir == null) {
    throw Error('must specify an output directory for the TextMate grammars');
  }

  // Key is a Monaco language; value is an Array of language configs.
  // For example, both typescript-basics and the json extension attempt to
  // define the "jsonc" language.
  const languageToLanguageConfigWithIndex = new Map<string, IndexedLanguageExtensionPoint[]>();

  // Key is a scopeName; value is an Array of scopeNames that want to extend
  // the key.
  const scopeNameToInjections = new Map<ScopeName, ScopeName[]>();
  const scopeNameToEmbeddedLanguages = new Map<ScopeName, Record<ScopeName, string>>();

  // Each element in this array is of type:
  const grammarResponses: GrammarResponse[] = [];
  await Promise.all(
    extensions.map(async (extension, index) => {
      const manifest = await extension.getManifest();

      const {contributes} = manifest;
      assert(contributes, `${extension} must contain 'contributes'`);
      const {grammars, languages} = contributes;
      assert(grammars, `${extension} must contain 'contributes.grammars'`);

      if (Array.isArray(languages)) {
        await Promise.all(
          languages.map(async languageConfig => {
            const {id} = languageConfig;
            let languageConfigList = languageToLanguageConfigWithIndex.get(id);
            if (languageConfigList == null) {
              languageConfigList = [];
              languageToLanguageConfigWithIndex.set(id, languageConfigList);
            }

            const normalizedLanguageConfig: NormalizedLanguageExtensionPoint =
              'configuration' in languageConfig
                ? {
                    ...languageConfig,
                    configuration: await extension.getLanguageConfiguration(
                      languageConfig.configuration,
                    ),
                  }
                : languageConfig;

            // Note we include the index so that we can sort languageConfigList
            // and ensure consistent output independent of the order in which
            // all of the `extension.getManifest()` promises resolve.
            languageConfigList.push({index, config: normalizedLanguageConfig});
          }),
        );
      } else {
        // eslint-disable-next-line no-console
        console.error(
          `Somewhat suspicious: package.json ${extension} does not include a 'contributes.languages' array.`,
        );
      }

      for (const languageConfigList of languageToLanguageConfigWithIndex.values()) {
        languageConfigList.sort((a, b) => a.index - b.index);
      }

      return Promise.all(
        grammars.map(async grammar => {
          const {scopeName, language = null, path, injectTo, embeddedLanguages = null} = grammar;
          assert(scopeName, `${extension} must contain scopeName for grammar`);
          assert(path, `${extension} must contain path for grammar`);

          const preferredExtension = EXTENSION_FOR_SCOPE_NAME.get(scopeName);
          if (preferredExtension != null && preferredExtension !== extension) {
            return;
          }

          if (language != null && IGNORED_LANGUAGES.has(language)) {
            return;
          }

          const grammarSource = await extension.getGrammar(path);
          grammarResponses.push({
            scopeName,
            language,
            ...grammarSource,
          });

          // At first glance, this may seem a bit dicey because if two grammars
          // have the same scope name, then only the embeddedLanguages from the
          // last entry will be present in the map. We rely on
          // writeGrammarFiles() to throw an exception if the same scope name
          // appears more than once, as the remediation is to update either
          // EXTENSION_FOR_SCOPE_NAME or IGNORED_LANGUAGES to fix the issue.
          if (embeddedLanguages != null) {
            scopeNameToEmbeddedLanguages.set(scopeName, embeddedLanguages);
          }

          if (Array.isArray(injectTo)) {
            for (const grammarToExtend of injectTo) {
              let injections = scopeNameToInjections.get(grammarToExtend);
              if (injections == null) {
                injections = [];
                scopeNameToInjections.set(grammarToExtend, injections);
              }
              injections.push(scopeName);
            }
          }
        }),
      );
    }),
  );

  // Remove index property from languageToLanguageConfigWithIndex.
  const languageToLanguageConfig = new Map<string, NormalizedLanguageExtensionPoint[]>();
  for (const [id, configWithIndex] of languageToLanguageConfigWithIndex.entries()) {
    languageToLanguageConfig.set(
      id,
      configWithIndex.map(record => record.config),
    );
  }

  await writeGrammarFiles(
    grammarResponses,
    languageToLanguageConfig,
    scopeNameToInjections,
    manifestPath,
    grammarsDir,
    scopeNameToEmbeddedLanguages,
  );
}

/**
 * This function writes files based on its inputs, but it will not perform any
 * network requests.
 */
async function writeGrammarFiles(
  grammarResponses: GrammarResponse[],
  languageToLanguageConfig: Map<string, NormalizedLanguageExtensionPoint[]>,
  scopeNameToInjections: Map<string, string[]>,
  manifestPath: string,
  grammarsDir: string,
  scopeNameToEmbeddedLanguages: Map<ScopeName, Record<ScopeName, string>>,
): Promise<void> {
  const __dirname = pathMod.resolve();
  const prettierOptions = await prettier.resolveConfig(__dirname);

  /**
   * @param filepath file to write
   * @param contents TypeScript source code to write (will be formatted by Prettier)
   */
  function createTypeScriptFile(filepath: string, contents: string): Promise<void> {
    const formattedContents = prettier.format(contents, {
      ...prettierOptions,
      filepath,
    });
    return fs.writeFile(filepath, formattedContents);
  }

  const scopeNameToGrammar = new Map();
  const grammarsManifest: GrammarLoadingInfo[] = [];
  // In the wild, we have seen multiple grammars lay claim to the same
  // Monaco language:
  //
  // - both source.cpp.embedded.macro and source.cpp associate themselves with `cpp`
  //
  // For now, we do not include the less common grammar, but we should see how
  // VS Code handles this natively and match it.
  await Promise.all(
    grammarResponses.map(async response => {
      const {scopeName, language = null, type, definition} = response;
      if (scopeNameToGrammar.has(scopeName)) {
        throw Error(`duplicate entry for scopeName ${scopeName}`);
      }

      scopeNameToGrammar.set(scopeName, response);

      const jsModule = `${scopeName.replace(/\./g, '_')}_TextMateGrammar`;
      const jsonFile = pathMod.join(grammarsDir, `${jsModule}.${type}`);
      const contents = type === 'json' ? JSON.stringify(definition) : definition;
      await fs.writeFile(jsonFile, contents);

      const injections = (scopeNameToInjections.get(scopeName) || []).slice().sort();

      const embeddedLanguages = scopeNameToEmbeddedLanguages.get(scopeName);
      grammarsManifest.push({
        scopeName,
        language,
        type,
        jsModule,
        injections,
        embeddedLanguages,
      });
    }),
  );
  // Sort by scopeName so that the output of this script is consistent.
  grammarsManifest.sort((a, b) => a.scopeName.localeCompare(b.scopeName));

  // languagesManifest is a map of monaco.languages.LanguageConfiguration-like
  // objects that can be serialized directly as JSON. Note that a real
  // monaco.languages.LanguageConfiguration can have RegExp values, but this
  // one uses strings instead.
  const languagesManifest: {[language: string]: NormalizedLanguageExtensionPoint} = {};
  const alphaSortedKeys = Array.from(languageToLanguageConfig.keys());
  alphaSortedKeys.sort();
  for (const language of alphaSortedKeys) {
    const languageConfig = languageToLanguageConfig.get(language);
    // By definition, every key in alphaSortedKeys is a key in
    // languageToLanguageConfig.
    assert(languageConfig != null);

    if (languageConfig.length === 1) {
      languagesManifest[language] = languageConfig[0];
    } else {
      // Currently, we do not have many cases where it is defined multiple
      // times (ini, json, and jsonc known examples), so our merge logic is
      // overly specific to these cases.
      const aggregateConfig: NormalizedLanguageExtensionPoint = {id: language};
      languageConfig.forEach(config => mergeLanguageConfig(config, aggregateConfig, language));
      languagesManifest[language] = aggregateConfig;
    }

    const fullConfig = languagesManifest[language];

    // eslint-disable-next-line no-prototype-builtins
    if (languageExtensionAmendments.hasOwnProperty(language)) {
      const amendments = languageExtensionAmendments[language];
      for (const [p, itemsToAdd] of Object.entries(amendments)) {
        const propertyName = p as keyof LanguageExtensionAmendment;
        // Special-case firstLine property, which should be a string rather
        // than a list.
        if (propertyName === 'firstLine') {
          const existingValue = fullConfig[propertyName];
          if (existingValue != null) {
            // eslint-disable-next-line no-console
            console.warn(
              `overwriting ${propertyName} for ${language}: '${existingValue}' => '${itemsToAdd}'`,
            );
          }

          Object.assign(fullConfig, {[propertyName]: itemsToAdd});
        } else {
          assert(Array.isArray(itemsToAdd), 'all other property values are of type string[]');

          const originalList = fullConfig[propertyName];
          const list = Array.isArray(originalList) ? originalList : (fullConfig[propertyName] = []);
          itemsToAdd.forEach(item => {
            if (list.indexOf(item) === -1) {
              list.push(item);
            }
          });
        }
      }
    }

    normalizeAutoClosingPairs(fullConfig);
  }

  const manifestSource = createTextMateGrammarManifest(grammarsManifest, languagesManifest);
  await createTypeScriptFile(manifestPath, manifestSource);
}

function createTextMateGrammarManifest(
  grammars: GrammarLoadingInfo[],
  languages: {[language: string]: NormalizedLanguageExtensionPoint},
): string {
  // For now, we will not write out the `configuration` property of each
  // NormalizedLanguageExtensionPoint because we do not need it unless we start
  // using TextMate grammars in a Monaco editor in the client.
  const filteredLanguages = Object.fromEntries(
    Object.entries(languages).map(([key, value]) => {
      const copy = {...value};
      // eslint-disable-next-line @typescript-eslint/ban-ts-comment
      // @ts-ignore
      delete copy.configuration;
      return [key, copy];
    }),
  );

  return `
export type TextMateGrammar = {
  type: 'json' | 'plist',
  /**
   * Grammar data as a string because parseRawGrammar() in vscode-textmate
   * takes the contents as a string, even if the type is json.
   */
  grammar: string,
};

type Grammar = {
  language?: string,
  injections: Array<string>,
  embeddedLanguages?: {[scopeName: string]: string},
  getGrammar: () => Promise<TextMateGrammar>,
};

const grammars: {[scopeName: string]: Grammar} = {
  ${grammars.map(createGrammarsEntry).join('')}
};

export type LanguageConfiguration = {
  id: string;
  extensions?: string[];
  filenames?: string[];
  filenamePatterns?: string[];
  firstLine?: string;
  aliases?: string[];
  mimetypes?: string[];
};

async function fetchGrammar(moduleName: string, type: 'json' | 'plist'): Promise<TextMateGrammar> {
  const uri = \`/generated/textmate/\${moduleName}.\${type}\`;
  const response = await fetch(uri);
  const grammar = await response.text();
  return {type, grammar};
}

const languages: {[language: string]: LanguageConfiguration} = ${JSON.stringify(
    filteredLanguages,
    undefined,
    2,
  )};

export {grammars, languages};
`;
}

function createGrammarsEntry(grammar: GrammarLoadingInfo) {
  const {scopeName, jsModule, type, language, injections, embeddedLanguages} = grammar;
  return `\
${JSON.stringify(scopeName)}: {
  language: ${language == null ? undefined : JSON.stringify(language)},
  injections: ${JSON.stringify(injections)},
  embeddedLanguages: ${JSON.stringify(embeddedLanguages)},
  getGrammar(): Promise<TextMateGrammar> {
    return fetchGrammar(${JSON.stringify(jsModule)}, ${JSON.stringify(type)});
  },
},
`;
}

/**
 * @param src NormalizedLanguageExtensionPoint to read from
 * @param dest NormalizedLanguageExtensionPoint to write to
 */
function mergeLanguageConfig(
  src: NormalizedLanguageExtensionPoint,
  dest: NormalizedLanguageExtensionPoint,
  language: string,
) {
  for (const [k, value] of Object.entries(src)) {
    const key = k as keyof NormalizedLanguageExtensionPoint;
    if (key === 'id') {
      assert(
        value === dest.id,
        `config with id ${value} should match dest id ${dest.id} for language ${language}`,
      );
      continue;
    }

    // eslint-disable-next-line no-prototype-builtins
    if (!dest.hasOwnProperty(key)) {
      Object.assign(dest, {[key]: value});
    } else {
      const destValue = dest[key];
      if (Array.isArray(destValue) && Array.isArray(value)) {
        destValue.push(...value);
      } else {
        throw Error(
          `do not know how to merge ${destValue} and ${value} for language ${language} with key ${key}`,
        );
      }
    }
  }
}

/**
 * Apparently VS Code allows the autoClosingPairs property to be specified as
 * an array where elements can be either:
 *
 * - a two element array of strings
 * - an object with open/close, and possibly a notIn property
 *
 * Some extensions even use both formats (e.g., Go):
 *
 * ```
 * autoClosingPairs: [
 *   ['{', '}'],
 *   ['[', ']'],
 *   ['(', ')'],
 *   {
 *     open: '`',
 *     close: '`',
 *     notIn: ['string'],
 *   },
 *   {
 *     open: '"',
 *     close: '"',
 *     notIn: ['string'],
 *   },
 *   {
 *     open: "'",
 *     close: "'",
 *     notIn: ['string', 'comment'],
 *   },
 * ],
 * ```
 *
 * Apparently the code we are using accepts only the object format and does not
 * normalize it for us, so we must do it ourselves.
 *
 * This function modifies config in place: it does not return anything.
 *
 * @param config to normalize
 */
function normalizeAutoClosingPairs(languageConfiguration: NormalizedLanguageExtensionPoint) {
  if (!('configuration' in languageConfiguration)) {
    return;
  }

  const {autoClosingPairs} = languageConfiguration.configuration;
  if (!Array.isArray(autoClosingPairs)) {
    return;
  }

  autoClosingPairs.forEach((value, index) => {
    if (Array.isArray(value)) {
      autoClosingPairs[index] = {open: value[0], close: value[1]};
    }
  });
}

main();
