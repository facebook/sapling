#debugruntest-compatible

This test is about `debugimportexport` without using the node IPC channel.


Test utility:

    import contextlib, subprocess, json
    @contextlib.contextmanager
    def ipc():
        proc = subprocess.Popen(['hg', 'debugimportexport'], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        def send(obj):
            proc.stdin.write(json.dumps(obj).encode() + b"\n")
            proc.stdin.flush()
            line = proc.stdout.readline()
            return json.loads(line.decode())
        try:
            yield send
        finally:
            proc.terminate()

Do nothing. Exit:

  $ newrepo
  >>> with ipc() as send:
  ...    send(['ping'])
  ...    send(['exit'])
  ['ok', 'ack']
  ['ok', None]

Create a commit, then read it out.

  $ newrepo
  >>> with ipc() as send:
  ...     commit1 = {
  ...         'author': 'test', 'date': [0, 0], 'text': 'P', 'parents': [],
  ...         'files': {'a.txt': {'data': 'aaa\n'}, 'b.txt': {'data': 'bbbbbb\n'}},
  ...     }
  ...     imported = send(['import', [['commit', {**commit1, 'mark': ':m1'}]]])
  ...     assert imported[0] == 'ok'
  ...     node = imported[1][':m1']
  ...     exported = send(['export', {'revs': ['%s', node]}])
  ...     assert exported[0] == 'ok'
  ...     for key in commit1:
  ...         assert exported[1][0][key] == commit1[key]

Write to working copy, then read it out.

  $ newrepo
  >>> with ipc() as send:
  ...     files = {'a.txt': {'data': 'aaa\n'}}
  ...     imported = send(['import', [['write', files]]])
  ...     assert imported[0] == 'ok'
  ...     exported = send(['export', {'revs': ['wdir()'], 'assumeTracked': ['a.txt']}])
  ...     assert exported[1][0]['files'] == files
  ...     exported = send(['export', {'revs': ['wdir()'], 'assumeTracked': ['a.txt'], 'sizeLimit': 0}])
  ...     assert 'dataRef' in exported[1][0]['files']['a.txt']
