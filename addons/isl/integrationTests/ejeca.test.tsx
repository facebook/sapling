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

describe('test killing process', () => {
  const sighandlerScript = `
const argv = process.argv;
const sleep = (waitTimeInMs) => new Promise(resolve => setTimeout(resolve, waitTimeInMs));

(async function main() {
let exitOnSigTerm = false;
let delay = 0;

if(argv.length > 2) {
    delay = parseInt(argv[2]);
    if(argv[argv.length - 1] !== "dontExitOnSigterm") {
        exitOnSigTerm = true;
    }
}

process.on('SIGTERM', () => {
    console.log("I was asked to stop politely");
    if(exitOnSigTerm) {
        process.exit(0)
    }
});

console.log("Hello");

for(let i=0; i < delay; i++) {
    await sleep(1000);
}

console.log("Goodbye");
})();
`;

  const spawnAndKill = async (
    pythonArgs: string[] = [],
    expectedOut: string = '',
    killArgs: Parameters<EjecaChildProcess['kill']> = [],
  ) => {
    let spawned = ejeca('node', ['-', ...pythonArgs], {
      input: sighandlerScript,
    });
    setTimeout(() => spawned.kill(...killArgs), 1000);
    expect((await spawned).stdout).toBe(expectedOut);
  };

  it('kill as sends sigterm by default', async () => {
    await spawnAndKill(['3', 'dontExitOnSigterm'], 'Hello\nI was asked to stop politely\nGoodbye');
  });

  it('sigkill can be set through force kill after a timeout', async () => {
    await spawnAndKill(['4', 'dontExitOnSigterm'], 'Hello\nI was asked to stop politely', [
      'SIGTERM',
      {forceKillAfterTimeout: 2000},
    ]);
  });

  it('sending sigkill just kills', async () => {
    await spawnAndKill(['100000000000'], 'Hello', ['SIGKILL']);
  });
});
