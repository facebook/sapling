/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import * as en from './en/common.json';
import EventEmitter from 'events';
import React, {createContext, useContext, useEffect, useState} from 'react';

/**
 * ISO 639-3 language code used to control which translation we use
 */
type LanguageId = string;

// TODO: these language files should be lazilly loaded rather than bundled with webpack
const langs: {[key: string]: {[key: string]: string}} = {
  en,
  // Add other languages here!
};

declare global {
  interface Window {
    // language may be pre-defined ahead of time by the HTML in the window
    saplingLanguage?: string;
  }
}

let currentLanguage: LanguageId = window.saplingLanguage ?? 'en';

const I18nContext = createContext(currentLanguage);
export const onChangeLanguage = new EventEmitter();

/**
 * We need to re-render translated components when the language is changed.
 * React context lets us easily re-render any component using the language.
 */
export function I18nSupport({children}: {children: React.ReactNode}) {
  const [lang, setLang] = useState(currentLanguage);
  useEffect(() => {
    onChangeLanguage.on('change', setLang);
    return () => void onChangeLanguage.removeListener('change', setLang);
  }, []);
  return <I18nContext.Provider value={lang}>{children}</I18nContext.Provider>;
}

export function getCurrentLanguage(): LanguageId {
  return currentLanguage;
}
export function setCurrentLanguage(lang: LanguageId) {
  currentLanguage = lang;
  onChangeLanguage.emit('change', currentLanguage);
}

/**
 * what key suffixes to use for count-based translations
 * e.g., in en, myStringKey_one = 'open {count} file', myStringKey_other = 'open {count} files'
 * so when you call t('myStringKey', {count: 4}) it uses the right plural.
 * Different languages have different rules for plurals.
 */
const pluralizers: {[key in keyof typeof langs]: (n: number) => string} = {
  en: (n: number) => (n == 1 ? 'one' : 'other'),
};

/**
 * Translate provided en-language string. All user-visible strings should use t() or <T></T>, including
 * title texts, tooltips, and error messages.
 * Generally, the parameter is taken to be the english translation directly.
 * You can also use a generic key and define an en translation.
 * ```
 * t('Cancel') -> 'Cancel' if current language is 'en',
 * t('Cancel') -> 'Abbrechen' if current language is 'de', etc
 * ```
 * To pluralize, pass a `count` option. Then define translations with keys according to the pluralization rules
 * in {@link pluralizers}.
 * ```
 * t('confirmFilesSave', {count: 1}) -> lookup en 'confirmFilesSave_one' -> 'Save 1 file' if current language is 'en'
 * t('confirmFilesSave', {count: 4}) -> lookup en 'confirmFilesSave_other' -> 'Save 4 files' if current language is 'en'
 * t('confirmFilesSave', {count: 4}) -> lookup de 'confirmFilesSave_other' -> 'Speichern Sie 4 Dateien' if current language is 'de'
 * ```
 * To include arbitrary opaque contents that are not translated, you can provide a replacer:
 * ```
 * t('Hello, my name is {name}.', {replace: {'{name}': getName()}})
 * ```
 * {@link T See also &lt;T&gt; React component}
 */
export function t(
  i18nKeyOrEnText: string,
  options?: {count?: number; replace?: {[key: string]: string}},
) {
  return translate(i18nKeyOrEnText, options).join('');
}

/**
 * Translates contents. Re-renders when language is updated.
 * {@link t See t() function documentation}
 *
 * Unlike `t()`, `options.replace` can include arbitrary `ReactNode` contents
 * ```
 * <T replace={{name: <b>{getName()}</b>}}>Hello, my name is $name.</T>
 * ```
 */
export function T({
  children,
  count,
  replace,
}: {
  children: string;
  count?: number;
  opaque?: ReactNode;
  replace?: {[key: string]: string | ReactNode};
}): JSX.Element {
  // trigger re-render if the langauge is changed
  useContext(I18nContext);

  return <>{translate(children, {count, replace})}</>;
}

function translate(
  i18nKeyOrEnText: string,
  options?: {count?: number; replace?: {[key: string]: string | ReactNode}},
): Array<string | ReactNode> {
  let result;
  if (options?.count != null) {
    const pluralized =
      getPlural(i18nKeyOrEnText, options.count, currentLanguage) ??
      // fallback to pluralized en if the currentLanguage doesn't have this key
      getPlural(i18nKeyOrEnText, options.count, 'en') ??
      // last resort is to use the key directly
      i18nKeyOrEnText;
    // replace number into the appropriate location
    result = pluralized.replace(/{count}/g, String(options.count));
  }
  if (!result) {
    result =
      langs[currentLanguage][i18nKeyOrEnText] ?? langs.en[i18nKeyOrEnText] ?? i18nKeyOrEnText;
  }
  if (options?.replace) {
    // if we split with a regexp match group, the value will stay in the array,
    // so it can be replaced later
    const regex = new RegExp(
      // this requires escaping so special characters like $ can be used
      '(' + Object.keys(options.replace).map(escapeForRegExp).join('|') + ')',
      'g',
    );
    const parts = result.split(regex);
    return (
      parts
        .map(part => options.replace?.[part] ?? part)
        // if we replace with a component, we need to set a key or react will complain
        .map((part, i) => (typeof part === 'object' ? {...part, key: i} : part))
    );
  }
  return [result];
}

function escapeForRegExp(s: string) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); // $& means the whole matched string
}

/**
 * Returns current language. Also triggers re-renders if the language is changed.
 */
export function useCurrentLang(): LanguageId {
  useContext(I18nContext);
  return currentLanguage;
}

function getPlural(i18nKeyOrEnText: string, count: number, lang: LanguageId): string | undefined {
  const pluralizer = pluralizers[lang];
  const key = i18nKeyOrEnText + '_' + pluralizer(count);
  return langs[lang][key];
}
