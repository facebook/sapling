/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type * as vscode from 'vscode';

import fs from 'fs';
import path from 'path';

const tryParse = (s: string): Record<string, unknown> | undefined => {
  try {
    return JSON.parse(s);
  } catch {
    return undefined;
  }
};
// VS Code requires a restart if you change the configured language,
// thus we can globally load this value on startup.
const nlsConfig = tryParse(process.env.VSCODE_NLS_CONFIG as string) as {locale: string} | undefined;
export const locale = validateLocale(nlsConfig?.locale) ?? 'en';

/** The locale will be inserted into the HTML of the webview as a JS variable,
 * so it's important that what we get from the environment variable
 * really is just a string locale. */
function validateLocale(l: string | undefined): string | undefined {
  if (l == null) {
    return undefined;
  }
  return /^[a-zA-Z_]+$/.test(l) ? l : undefined;
}

let translations: {[key: string]: string} | undefined = undefined;

/**
 * Load translations for configured language from disk.
 * Should be called in extension's activate() method before
 * any calls to `t()`.
 */
export async function ensureTranslationsLoaded(
  extensionContext: vscode.ExtensionContext,
): Promise<void> {
  try {
    const translationsData = await fs.promises.readFile(
      path.join(
        extensionContext.extensionPath,
        locale === 'en' ? 'package.nls.json' : `package.nls.${locale}.json`,
      ),
      'utf-8',
    );
    translations = JSON.parse(translationsData);
  } catch (err) {
    // eslint-disable-next-line no-console
    console.error(`failed to load translation data for locale ${locale}: ${err}`);
  }
}

/**
 * Internationalize (i18n-ize) a string from a given key.
 * This implementation is specifically for the vscode extension.
 * The webview has its own i18n system which can hook into the
 * VS Code configured langauge.
 * This function is used for UI-visible text from the extension host.
 * The translations live in package.nls.*.json files,
 * which includes translations for values within package.json,
 * such as command names. These nls files are included in the
 * distributed extension VSIX, but are not bundled directly into the JS by webpack.
 */
export function t(key: string): string {
  return translations?.[key] ?? key;
}
