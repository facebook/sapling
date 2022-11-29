/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type * as vscode from 'vscode';

// don't want to mock vscode.Uri, so use library for it
import * as vscodeUri from 'vscode-uri';
export const Uri = vscodeUri.URI;

export const workspace = proxyMissingFieldsWithJestFn({
  workspaceFolders: undefined,
  getConfiguration: () => ({get: jest.fn()}),
});
export const scm = proxyMissingFieldsWithJestFn({
  createSourceControl: jest.fn(
    (): vscode.SourceControl => ({
      inputBox: {value: '', placeholder: '', enabled: true, visible: true},
      createResourceGroup: jest.fn(() => ({
        hideWhenEmpty: false,
        resourceStates: [],
        id: '',
        label: '',
        dispose: jest.fn(),
      })),
      id: '',
      dispose: jest.fn(),
      label: '',
      rootUri: Uri.file(''),
    }),
  ),
});

export class ThemeColor {
  constructor(public id: string) {}
}

export class Disposable implements vscode.Disposable {
  dispose = jest.fn();
}

// to avoid manually writing jest.fn() for every API,
// assume fields that we don't provide are jest.fn() which return disposables
function proxyMissingFieldsWithJestFn<T extends object>(t: T): T {
  return new Proxy(t, {
    get: ((_: unknown, key: keyof T) => {
      if (Object.prototype.hasOwnProperty.call(t, key)) {
        return t[key];
      }
      // make sure we keep the jest.fn() we make so it's not remade each time
      t[key] = jest.fn().mockReturnValue(new Disposable()) as unknown as typeof t[keyof T];
      return t[key];
    }) as unknown as ProxyHandler<T>['get'],
  });
}
