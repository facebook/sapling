#debugruntest-compatible

  $ configure modern

Test utils:

  $ cat > pprint.py << 'EOS'
  > import json, pprint, sys
  > obj = json.load(sys.stdin)
  > s = pprint.pformat(obj, width=200)  # pformat is more compact than json
  > sys.stdout.buffer.write((s + "\n").encode())
  > EOS
  $ pprint() {
  >   python ~/pprint.py
  > }

Export a linear stack of various kinds of files: modified, renamed, deleted,
non-utf8, symlink, executable:

  $ newrepo
  $ drawdag << 'EOS'
  > A..D
  > python:
  > commit('A', remotename='remote/master', files={"A":"1"})
  > commit('B', files={"A":"2", "B":"3 (executable)"})
  > commit('C', files={"C":b85(b"\xfbm"), "Z": "B (symlink)"})  # C: invalid utf-8
  > commit('D', files={"D":"2 (renamed from A)", "E": "E (copied from C)"})
  > EOS

  $ hg debugexportstack -r $B::$D | pprint
  [{'author': 'test', 'date': [0.0, 0], 'immutable': True, 'node': '983f771099bbf84b42d0058f027b47ede52f179a', 'relevant_files': {'A': {'data': '1'}, 'B': None}, 'requested': False, 'text': 'A'},
   {'author': 'test',
    'date': [0.0, 0],
    'files': {'A': {'data': '2'}, 'B': {'data': '3', 'flags': 'x'}},
    'immutable': False,
    'node': '8b5b077308ecdd37270b7b94d98d64d27c170dfb',
    'parents': ['983f771099bbf84b42d0058f027b47ede52f179a'],
    'relevant_files': {'C': None, 'Z': None},
    'requested': True,
    'text': 'B'},
   {'author': 'test',
    'date': [0.0, 0],
    'files': {'C': {'data_base85': "b'`)v'"}, 'Z': {'data': 'B', 'flags': 'l'}},
    'immutable': False,
    'node': 'd2a2ca8387f2339934b6ce3fb17992433e06fdd4',
    'parents': ['8b5b077308ecdd37270b7b94d98d64d27c170dfb'],
    'relevant_files': {'A': {'data': '2'}, 'D': None, 'E': None},
    'requested': True,
    'text': 'C'},
   {'author': 'test',
    'date': [0.0, 0],
    'files': {'A': None, 'D': {'copy_from': 'A', 'data': '2'}, 'E': {'copy_from': 'C', 'data': 'E'}},
    'immutable': False,
    'node': 'f5086e168b2741946a5118463a8be38273822529',
    'parents': ['d2a2ca8387f2339934b6ce3fb17992433e06fdd4'],
    'requested': True,
    'text': 'D'}]

Various kinds of limits:

  $ hg debugexportstack -r $B::$D --config experimental.exportstack-max-commit-count=2
  {"error": "too many commits"}
  [1]
  $ hg debugexportstack -r $B::$D --config experimental.exportstack-max-file-count=2
  {"error": "too many files"}
  [1]
  $ hg debugexportstack -r $B::$D --config experimental.exportstack-max-bytes=4B
  {"error": "too much data"}
  [1]

