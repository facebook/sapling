/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type * as vscode from 'vscode';

const readFile = jest.fn();

jest.mock('node:fs', () => ({
  __esModule: true,
  default: {
    promises: {readFile},
  },
}));

const originalNlsConfig = process.env.VSCODE_NLS_CONFIG;

function loadI18n(locale: string | undefined) {
  jest.resetModules();
  if (locale == null) {
    delete process.env.VSCODE_NLS_CONFIG;
  } else {
    process.env.VSCODE_NLS_CONFIG = JSON.stringify({locale});
  }
  return import('../i18n');
}

afterEach(() => {
  readFile.mockReset();
});

afterAll(() => {
  if (originalNlsConfig == null) {
    delete process.env.VSCODE_NLS_CONFIG;
  } else {
    process.env.VSCODE_NLS_CONFIG = originalNlsConfig;
  }
});

it('uses bundled English translations without reading from disk', async () => {
  const i18n = await loadI18n('en');

  await i18n.ensureTranslationsLoaded({extensionPath: '/extension'} as vscode.ExtensionContext);

  expect(readFile).not.toHaveBeenCalled();
  expect(i18n.t('isl.title')).toBe('Interactive Smartlog');
});

it('loads non-English translations dynamically', async () => {
  readFile.mockResolvedValue(JSON.stringify({'isl.title': 'Smartlog traduit'}));
  const i18n = await loadI18n('fr');

  await i18n.ensureTranslationsLoaded({extensionPath: '/extension'} as vscode.ExtensionContext);

  expect(readFile).toHaveBeenCalledWith('/extension/package.nls.fr.json', 'utf-8');
  expect(i18n.t('isl.title')).toBe('Smartlog traduit');
});
