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
export const webviewPlatform: Platform = {
  ...browserPlatform, // just act like the browser platform by default, since the app use case is similar
  platformName: 'webview',
  openExternalLink(url: string) {
    invoke({cmd: 'openExternal', url});
  },
  confirm(message: string, details?: string): Promise<boolean> {
    return request({cmd: 'confirm', message, details}).then(({ok}) => ok);
  },
  async chooseFile(title: string, multi: boolean): Promise<Array<File>> {
    const response = await request({cmd: 'chooseFile', title, path: '', multi, mediaOnly: true});
    const {files} = response;
    if (!files) {
      return [];
    }
    const result = files.map(value => b64toFile(value.base64Content, value.name));
    return result;
  },
};

function b64toFile(b64Data: string, filename: string, sliceSize = 512): File {
  const byteCharacters = atob(b64Data);
  const byteArrays = [];

  for (let offset = 0; offset < byteCharacters.length; offset += sliceSize) {
    const slice = byteCharacters.slice(offset, offset + sliceSize);

    const byteNumbers = new Array(slice.length);
    for (let i = 0; i < slice.length; i++) {
      byteNumbers[i] = slice.charCodeAt(i);
    }

    const byteArray = new Uint8Array(byteNumbers);
    byteArrays.push(byteArray);
  }

  const blobParts = [new Blob(byteArrays)];
  const file = new File(blobParts, filename);
  return file;
}

window.islPlatform = webviewPlatform;

if (navigator.platform.toLowerCase().includes('mac')) {
  // Handle missing shortcuts & events on macOS.
  // See https://github.com/webview/webview/issues/403
  window.addEventListener('keypress', event => {
    const onlyMeta = event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
    if (onlyMeta && event.key === 'c') {
      document.execCommand('copy');
      event.preventDefault();
    }
    if (onlyMeta && event.key === 'v') {
      event.preventDefault();
      // Weirdly, this causes a small context menu popup that has the "paste" option.
      // Clicking this does indeed do the paste, which includes support for images, etc.
      // I can't find why it happens this way, but I assume it's related to security.
      // I think this needs to be fixed in the webview library itself.
      // See also https://github.com/webview/webview/issues/397, https://github.com/webview/webview/issues/403
      document.execCommand('paste');
    }
    if (onlyMeta && event.key === 'x') {
      event.preventDefault();
      document.execCommand('cut');
    }
    if (onlyMeta && event.key === 'a') {
      event.preventDefault();
      document.execCommand('selectAll');
    }
    if (onlyMeta && event.key === 'z') {
      event.preventDefault();
      document.execCommand('undo');
    }
    if (event.metaKey && event.shiftKey && !event.ctrlKey && !event.altKey && event.key === 'z') {
      event.preventDefault();
      document.execCommand('redo');
    }
  });
}

/**
 * Typed commands to communicate from the frontend with the Rust app hosting the webview.
 * This should match the rust types used in webview-app.
 */
type ExternalWebviewCommandsInvoke =
  | {cmd: 'openExternal'; url: string}
  | {cmd: 'confirm'; message: string; details?: string}
  | {cmd: 'chooseFile'; title: string; path: string; multi: boolean; mediaOnly: boolean};
type ExternalWebviewCommandsResponse = (
  | {cmd: 'confirm'; ok: boolean}
  | {
      cmd: 'chooseFile';
      files: Array<{
        name: string;
        base64Content: string;
      }>;
    }
) & {id: number};

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
