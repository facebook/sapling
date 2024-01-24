/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

const {spawn, spawnSync} = require('child_process');

const example = process.argv[2] ?? 'hello_child';
const build = spawnSync('cargo', ['build', '--message-format=json', '--release', '--example', example], {stdio: [0, 'pipe', 2]});
const output = build.stdout.toString();

let executable = null;
output.split('\n').forEach(line => {
  if (line && line.indexOf('executable') > 0) {
    const obj = JSON.parse(line);
    executable ??= obj?.executable;
  }
});

const stdin_opt = process.argv.includes('--stdin=pipe') ? 'pipe': 'inherit';

const child = spawn(executable, {stdio: [stdin_opt, 'inherit', 'inherit', 'ipc']} );
const responses = ['HELLO FROM PARENT 1', 'HELLO FROM PARENT 2', 'BYE'];
if (stdin_opt === 'pipe') {
  child.stdin.write('stdin content from node');
  child.stdin.end();
}
child.on('message', message => {
  console.log('[Parent] Got message from child:', message);
  const response = responses.shift();
  if (response) {
    child.send(response);
  }
});
child.on('exit', () => {
  console.log('[Parent] Child process has exited');
});

/* Example output:

[Parent] Got message from child: HELLO FROM CHILD
[Child] Got message from parent: String("HELLO FROM PARENT 1")
[Parent] Got message from child: [ 'Echo from child', 'HELLO FROM PARENT 1' ]
[Child] Got message from parent: String("HELLO FROM PARENT 2")
[Parent] Got message from child: [ 'Echo from child', 'HELLO FROM PARENT 2' ]
[Child] Got message from parent: String("BYE")
[Parent] Child process has exited

*/
