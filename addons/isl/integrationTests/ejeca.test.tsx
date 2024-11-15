/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {EjecaChildProcess} from 'shared/ejeca';
import {ejeca} from 'shared/ejeca';

describe('test running binaries', () => {
  it('we can get both streams and awaitables', async () => {
    let spawned = ejeca('node', ['-e', "console.log('uno') ; console.error('dos')"]);
    let streamOut = '';
    let streamErr = '';
    spawned.stdout?.on('data', data => {
      streamOut = data.toString();
    });
    spawned.stderr?.on('data', data => {
      streamErr = data.toString();
    });
    const result = await spawned;
    expect(result.stdout).toBe('uno');
    expect(streamOut).toBe('uno\n');
    expect(result.stderr).toBe('dos');
    expect(streamErr).toBe('dos\n');
  });

  it('we can set pass stdin as a string', async () => {
    const spawned = ejeca('node', [], {input: 'console.log("hemlo")'});
    expect((await spawned).stdout).toBe('hemlo');
  });
});
