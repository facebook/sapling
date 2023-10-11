/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from '../platform';

import {browserPlatform} from '../BrowserPlatform';

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated by webpack.

/**
 * This platform is used when spawned as a standalone webview from `sl web`.
 * We pass messages to the rust side via `external.invoke`,
 * with JSON serialized requests. Rust will respond back with JSON serialized responses.
 * This lets us handle features like alerts, file dialogs, and opening external links
 * which are not implemented in the webview itself.
 */
const webviewPlatform: Platform = {
  ...browserPlatform, // just act like the browser platform by default, since the app use case is similar
  platformName: 'webview',
  openExternalLink(url: string) {
    invoke({cmd: 'openExternal', url});
  },
  confirm(message: string, details?: string): Promise<boolean> {
    return request({cmd: 'confirm', message, details}).then(({ok}) => ok);
  },
};

window.islPlatform = webviewPlatform;

/**
 * Typed commands to communicate from the frontend with the Rust app hosting the webview.
 * This should match the rust types used in webview-app.
 */
type ExternalWebviewCommandsInvoke =
  | {cmd: 'openExternal'; url: string}
  | {cmd: 'confirm'; message: string; details?: string};
type ExternalWebviewCommandsResponse = {cmd: 'confirm'} & {ok: boolean; id: number};

declare global {
  interface Window {
    islWebviewHandleResponse: (response: ExternalWebviewCommandsResponse) => void;
  }
}

let nextId = 0;
const callbacks: Array<(response: ExternalWebviewCommandsResponse) => void> = [];
window.islWebviewHandleResponse = (response: ExternalWebviewCommandsResponse) => {
  const cb = callbacks[response.id];
  if (cb) {
    cb(response);
    delete callbacks[response.id];
  }
};

declare const external: {
  invoke(arg: string): Promise<void>;
};

function invoke(json: ExternalWebviewCommandsInvoke) {
  external.invoke(JSON.stringify({...json, id: nextId++}));
}

function request<K extends ExternalWebviewCommandsInvoke['cmd']>(
  json: ExternalWebviewCommandsInvoke & {cmd: K},
): Promise<ExternalWebviewCommandsResponse & {cmd: K}> {
  const id = nextId++;
  let resolve: (value: ExternalWebviewCommandsResponse & {cmd: K}) => void;
  const callback = (response: ExternalWebviewCommandsResponse) => {
    resolve(response as ExternalWebviewCommandsResponse & {cmd: K});
  };
  const promise = new Promise<ExternalWebviewCommandsResponse & {cmd: K}>(res => {
    resolve = res;
  });
  external.invoke(JSON.stringify({...json, id}));
  callbacks[id] = callback;

  return promise;
}
