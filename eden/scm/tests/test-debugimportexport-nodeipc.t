#debugruntest-compatible
#require node

This test is about `debugimportexport --node-ipc`.

Node module with utilities for testing:

  $ cat > ipc.mjs << 'EOF'
  > import { spawn } from 'node:child_process';
  > export const withIpc = async (callback) => {
  >     const proc = spawn('hg', ['debugimportexport', '--node-ipc'], {stdio: [0, 1, 2, 'ipc']});
  >     const sendRecv = (obj) => {
  >         proc.send(obj);
  >         return new Promise((resolve,) => proc.once('message', resolve));
  >     };
  >     try {
  >         await callback({sendRecv});
  >     } finally {
  >         proc.kill();
  >     }
  > };
  > EOF

Do nothing. Exit:

  $ newrepo
  $ node --input-type=module << 'EOF'
  > import { withIpc } from '../ipc.mjs';
  > import * as assert from 'node:assert';
  > await withIpc(async ({sendRecv}) => {
  >     const pingResponse = await sendRecv(['ping']);
  >     assert.deepEqual(pingResponse, ['ok', 'ack']);
  >     const exitResponse = await sendRecv(['exit']);
  >     assert.deepEqual(exitResponse, ['ok', null]);
  > });
  > EOF

Create a commit, then read it out.

  $ newrepo
  $ node --input-type=module << 'EOF'
  > import { withIpc } from '../ipc.mjs';
  > import * as assert from 'node:assert';
  > const commit1 = {
  >     author: 'test', date: [0, 0], text: 'P', parents: [],
  >     files: {'a.txt': {data: 'aaa\n'}, 'b.txt': {data: 'bbbbbb\n'}},
  > }
  > await withIpc(async ({sendRecv}) => {
  >     const imported = await sendRecv(['import', [['commit', {...commit1, mark: ':m1'}]]]);
  >     assert.deepEqual(imported[0], 'ok');
  >     const node = imported[1][':m1'];
  >     const exported = await sendRecv(['export', {revs: ['%s', node]}]);
  >     assert.deepEqual(exported, ['ok', [{...commit1, node, immutable: false, requested: true}]]);
  > });
  > EOF

Write to working copy, then read it out.

  $ newrepo
  $ node --input-type=module << 'EOF'
  > import { withIpc } from '../ipc.mjs';
  > import * as assert from 'node:assert';
  > const files = {'a.txt': {data: 'aaa\n'}};
  > await withIpc(async ({sendRecv}) => {
  >     await sendRecv(['import', [['write', files]]]);
  >     let exported = await sendRecv(['export', {revs: ['wdir()'], assumeTracked: ['a.txt']}]);
  >     assert.deepEqual(exported[1][0].files, files);
  >     // Use sizeLimit to force dataRef.
  >     exported = await sendRecv(['export', {revs: ['wdir()'], assumeTracked: ['a.txt'], 'sizeLimit': 0}]);
  >     assert.deepEqual(exported[1][0].files['a.txt'], {dataRef: {node: 'ff'.repeat(20), path: 'a.txt'}});
  > });
  > EOF
