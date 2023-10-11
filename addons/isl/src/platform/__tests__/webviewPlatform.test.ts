/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {webviewPlatform} from '../webviewPlatform';
import {unwrap} from 'shared/utils';

(window.external as unknown as Record<string, unknown>).invoke = jest.fn();

describe('webview platform', () => {
  const external = () => {
    return window.external as unknown as {invoke: jest.FunctionLike};
  };

  it('can send openExternal messages', () => {
    webviewPlatform.openExternalLink('example.com');
    expect(external().invoke).toHaveBeenCalledWith(
      '{"cmd":"openExternal","url":"example.com","id":0}',
    );
  });

  it('can send request and receive response messages', async () => {
    const promise = webviewPlatform.chooseFile?.('my title', true);
    expect(external().invoke).toHaveBeenCalledWith(
      '{"cmd":"chooseFile","title":"my title","path":"","multi":true,"mediaOnly":true,"id":1}',
    );

    const msg = 'Hello';
    const msg_b64 = Buffer.from(msg).toString('base64');
    expect(msg_b64).toEqual('SGVsbG8=');
    const msg_bytes = Buffer.from(msg);
    expect(msg_bytes).toEqual(Buffer.from(new Uint8Array([72, 101, 108, 108, 111])));
    window.islWebviewHandleResponse({
      cmd: 'chooseFile',
      files: [{name: 'file.txt', base64Content: msg_b64}],
      id: 1,
    });

    const result = unwrap(await promise);
    expect(result[0].name).toEqual('file.txt');
    expect(await result[0].size).toEqual(5);
  });
});
