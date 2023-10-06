#debugruntest-compatible

  $ configure modern
  $ enable tweakdefaults
  $ setconfig tweakdefaults.showupdated=true

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

Python test utils:

    @command
    def marks(args, stdin, stdout, fs, marks={}):
        """Maintains 'marks'. Can be used to get or set marks->hashes.
        Use 'marks :1 :2' to convert marks to hex hashes in JSON.
        Use 'hg debugimportstack ... | marks' to track marks outputted from hg.
        Use 'hg ... | marks' to convert hex commit hashes back to marks.
        """
        import json
        input_bytes = stdin.read()
        if input_bytes:
            if input_bytes.startswith(b"{"):
                obj = json.loads(input_bytes.decode().splitlines()[0])
                marks.update(obj)
            else:
                for m, n in marks.items():
                    input_bytes = input_bytes.replace(n.encode(), m.encode())
            stdout.write(input_bytes)
        if args:
            stdout.write(json.dumps([marks[mark] for mark in args]).encode())

Export a linear stack of various kinds of files: modified, renamed, deleted,
non-utf8, symlink, executable:

  $ newrepo
  $ drawdag << 'EOS'
  > A..D
  > python:
  > commit('A', remotename='remote/master', files={"A":"1"})
  > commit('B', files={"A":"2", "B":"3 (executable)"})
  > commit('C', files={"C":b85(b"\xfbm"), "Z": "B (symlink)"})  # C: invalid utf-8
  > commit('D', files={"D":"222 (renamed from A)", "E": "E (copied from C)"})
  > EOS

Test that various code paths in debugexportstack are exercised:

    from sapling.commands import debugstack
    with assertCovered(debugstack.debugexportstack, debugstack._export):
      # Regular export.
      $ hg debugexportstack -r $B::$D | pprint
      [{'author': 'test', 'date': [0.0, 0], 'immutable': True, 'node': '983f771099bbf84b42d0058f027b47ede52f179a', 'relevantFiles': {'A': {'data': '1'}, 'B': None}, 'requested': False, 'text': 'A'},
       {'author': 'test',
        'date': [0.0, 0],
        'files': {'A': {'data': '2'}, 'B': {'data': '3', 'flags': 'x'}},
        'immutable': False,
        'node': '8b5b077308ecdd37270b7b94d98d64d27c170dfb',
        'parents': ['983f771099bbf84b42d0058f027b47ede52f179a'],
        'relevantFiles': {'C': None, 'Z': None},
        'requested': True,
        'text': 'B'},
       {'author': 'test',
        'date': [0.0, 0],
        'files': {'C': {'dataBase85': '`)v'}, 'Z': {'data': 'B', 'flags': 'l'}},
        'immutable': False,
        'node': 'd2a2ca8387f2339934b6ce3fb17992433e06fdd4',
        'parents': ['8b5b077308ecdd37270b7b94d98d64d27c170dfb'],
        'relevantFiles': {'A': {'data': '2'}, 'D': None, 'E': None},
        'requested': True,
        'text': 'C'},
       {'author': 'test',
        'date': [0.0, 0],
        'files': {'A': None, 'D': {'copyFrom': 'A', 'data': '222'}, 'E': {'copyFrom': 'C', 'data': 'E'}},
        'immutable': False,
        'node': '2a493edf8997358199a3bb8b486fc77798cd39a4',
        'parents': ['d2a2ca8387f2339934b6ce3fb17992433e06fdd4'],
        'requested': True,
        'text': 'D'}]

      # Use "dataRef" for large files (file "D" in commit "D").
      $ hg debugexportstack -r $B::$D --config experimental.exportstack-max-bytes=2B | pprint
      [{'author': 'test', 'date': [0.0, 0], 'immutable': True, 'node': '983f771099bbf84b42d0058f027b47ede52f179a', 'relevantFiles': {'A': {'data': '1'}, 'B': None}, 'requested': False, 'text': 'A'},
       {'author': 'test',
        'date': [0.0, 0],
        'files': {'A': {'data': '2'}, 'B': {'data': '3', 'flags': 'x'}},
        'immutable': False,
        'node': '8b5b077308ecdd37270b7b94d98d64d27c170dfb',
        'parents': ['983f771099bbf84b42d0058f027b47ede52f179a'],
        'relevantFiles': {'C': None, 'Z': None},
        'requested': True,
        'text': 'B'},
       {'author': 'test',
        'date': [0.0, 0],
        'files': {'C': {'dataBase85': '`)v'}, 'Z': {'data': 'B', 'flags': 'l'}},
        'immutable': False,
        'node': 'd2a2ca8387f2339934b6ce3fb17992433e06fdd4',
        'parents': ['8b5b077308ecdd37270b7b94d98d64d27c170dfb'],
        'relevantFiles': {'A': {'data': '2'}, 'D': None, 'E': None},
        'requested': True,
        'text': 'C'},
       {'author': 'test',
        'date': [0.0, 0],
        'files': {'A': None, 'D': {'copyFrom': 'A', 'dataRef': {'node': '2a493edf8997358199a3bb8b486fc77798cd39a4', 'path': 'D'}}, 'E': {'copyFrom': 'C', 'data': 'E'}},
        'immutable': False,
        'node': '2a493edf8997358199a3bb8b486fc77798cd39a4',
        'parents': ['d2a2ca8387f2339934b6ce3fb17992433e06fdd4'],
        'requested': True,
        'text': 'D'}]

      # Export the working copy.
      $ hg go -q $D
      $ echo 3 > D
      $ echo X > X
      $ rm C
      $ hg addremove -q X
      $ hg mv B B1
      $ echo F > F
      $ echo G > G
      $ hg debugexportstack -r 'wdir()' --assume-tracked B1 --assume-tracked C --assume-tracked G | pprint
      [{'author': 'test',
        'date': [0.0, 0],
        'immutable': False,
        'node': '2a493edf8997358199a3bb8b486fc77798cd39a4',
        'relevantFiles': {'B': {'data': '3', 'flags': 'x'}, 'B1': None, 'C': {'dataBase85': '`)v'}, 'D': {'copyFrom': 'A', 'data': '222'}, 'G': None, 'X': None},
        'requested': False,
        'text': 'D'},
       {'author': 'test',
        'date': [0, 0],
        'files': {'B': None, 'B1': {'copyFrom': 'B', 'data': '3', 'flags': 'x'}, 'C': None, 'D': {'data': '3\n'}, 'G': {'data': 'G\n'}, 'X': {'data': 'X\n'}},
        'immutable': False,
        'node': 'ffffffffffffffffffffffffffffffffffffffff',
        'parents': ['2a493edf8997358199a3bb8b486fc77798cd39a4'],
        'requested': True,
        'text': ''}]

Import stack:

    with assertCovered(
      debugstack.debugimportstack,
      debugstack._create_commits,
      debugstack._filectxfn,
      debugstack._reset,
      debugstack._write_files,
      debugstack._import,
    ):
      # Simple linear stack
        $ newrepo
        $ hg debugimportstack << EOS | marks
        > [["commit", {"author": "test1", "date": [3600, 3600], "text": "A", "mark": ":1", "parents": ["."],
        >   "files": {"A": {"data": "A"}}}],
        >  ["commit", {"author": "test2", "date": [7200, 0], "text": "B", "mark": ":2", "parents": [":1"],
        >   "files": {"B": {"dataBase85": "LNN", "flags": "l"}}}],
        >  ["commit", {"author": "test3", "date": [7200, -3600], "text": "C", "mark": ":3", "parents": [":2"],
        >   "files": {"A": null, "C": {"data": "C1", "copyFrom": "A", "flags": "x"}}}],
        >  ["goto", {"mark": ":3"}]
        > ]
        > EOS
        {":1": "8e5dcdd5f19d443087e9916eecdac0505203e7c8", ":2": "c39ea291adacb1e3e0836ae80754a1bcff7bf9bc", ":3": "b32f0b24ea604d28def8eaf7730c4167ea79b35f"}

      # Check file contents and commit graph

        if hasfeature("execbit"):
          $ f -m C
          C: mode=755
        if hasfeature("symlink"):
          $ f B
          B -> B1

        $ cat C
        C1 (no-eol)

        $ hg log -Gr 'all()' -T '{desc} {author} {date|isodate}'
        @  C test3 1970-01-01 03:00 +0100
        │
        o  B test2 1970-01-01 02:00 +0000
        │
        o  A test1 1970-01-01 00:00 -0100

      # Fold

        $ hg debugimportstack << EOS | marks
        > [["commit", {"text": "D", "mark": ":4",
        >   "parents": [], "predecessors": `marks :1 :2 :3`, "operation": "fold",
        >   "files": {"D": {"data": "D"}}}],
        >  ["goto", {"mark": ":4"}]]
        > EOS
        {":4": "058c1e1fb10a795a64351fb098ef497ea1b2ddbb"}

        $ ls
        D

        $ hg log -Gr 'all()' -T '{desc}'
        @  D

        $ hg debugmutation -r 'all()' | marks
         *  :4 fold by test at 1970-01-01T00:00:00 from:
            |-  :1
            |-  :2
            '-  :3

        $ hg hide 'desc(D)' -q

      # Split E -> [E1, E2, E3], and amend E -> E4, then reset

        $ hg debugimportstack << EOS | marks
        > [["commit", {"text": "E", "mark": ":5"}],
        >  ["commit", {"text": "E1", "mark": ":5a", "predecessors": [":5"]}],
        >  ["commit", {"text": "E2", "mark": ":5b", "predecessors": [":5"], "parents": [":5a"]}],
        >  ["commit", {"text": "E3", "mark": ":5c", "predecessors": [":5"], "parents": [":5b"], "operation": "split"}],
        >  ["commit", {"text": "E4", "mark": ":5d", "predecessors": [":5"], "operation": "amend"}],
        >  ["reset", {"mark": ":5c"}]]
        > EOS
        {":5": "163d5eee69569f6c170b946217ad981a726953ae", ":5a": "2a9f073f64d6ea3c1f8fd101515a7fb25cc1a20e", ":5b": "7eaade15648c4bd75f9884135ec311793ac5da01", ":5c": "9c69a6a007b9a6943f24635f36c1ad96b1feb8e2", ":5d": "4696154a532aa02b935321014b1ae9a61f94faea"}

      # Reset preserves "D" from the last "goto".

        $ ls

      # E should be hidden.

        $ hg log -Gr 'all()' -T '{desc}'
        o  E4
        
        @  E3
        │
        o  E2
        │
        o  E1

      # Check the mutation graph:
      # E1 (:5a) and E2 (:5b) should not have predecessor set.
      # E3 (:5c) should have "split into" information about E1 (:5a) and E2 (:5b).
      # E4 (:5c) should not have "split into" information.

        $ hg debugmutation -r 'all()' | marks
         *  :5a
        
         *  :5b
        
         *  :5c split by test at 1970-01-01T00:00:00 (split into this and: :5a, :5b) from:
            :5
        
         *  :5d amend by test at 1970-01-01T00:00:00 from:
            :5

      # Hide.

        $ hg up -q 'bottom()'
        $ hg debugimportstack << EOS | marks
        > [["hide", {"nodes": `marks :5b :5c :5d`}]]
        > EOS
        {}

        $ hg log -Gr 'all()' -T '{desc}'
        @  E1

      # Refer to working copy content.
      # (add untracked file, add renamed file, delete file)

        $ echo content > x
        $ hg debugimportstack << EOS | marks
        > [["commit", {"text": "F", "mark": ":6", "files": {"x": "."}}],
        >  ["goto", {"mark": ":6"}]]
        > EOS
        {":6": "d10c8b78105441f6dc32a7bbce4168ea9c65f1e1"}

        $ hg mv x y
        $ hg debugimportstack << EOS | marks
        > [["commit", {"text": "G", "mark": ":7", "files": {"y": "."}, "parents": ["."]}],
        >  ["goto", {"mark": ":7"}]]
        > EOS
        {":7": "21c37e7dcec01cab99284455a842e5c1f4dc1023"}

        $ rm y

        $ hg debugimportstack << EOS | marks
        > [["commit", {"text": "H", "mark": ":8", "files": {"y": "."}, "parents": `marks :7`}],
        >  ["goto", {"mark": ":8"}]]
        > EOS
        {":8": "9d8fe7c75ea2d88aa6e3242283443a8904991ed7"}

        $ hg log -p -T '{desc}\n' -fr . --config diff.git=true
        H
        diff --git a/y b/y
        deleted file mode 100644
        --- a/y
        +++ /dev/null
        @@ -1,1 +0,0 @@
        -content
        
        G
        diff --git a/x b/y
        copy from x
        copy to y
        
        F
        diff --git a/x b/x
        new file mode 100644
        --- /dev/null
        +++ b/x
        @@ -0,0 +1,1 @@
        +content

      # Refer to working copy copyFrom.

        $ hg mv x x1
        $ hg debugimportstack << EOS | marks
        > [["commit", {"text": "I", "mark": ":9", "files": {"x1": {"data": "x1\n", "copyFrom": "."}}, "parents": `marks :8`}],
        >  ["goto", {"mark": ":9"}]]
        > EOS
        {":9": "f01615cc474d26aa1116ce3528c5d5f5f9651c89"}

        $ hg status --copies --change .
        A x1
          x
        $ hg cat -r . x1
        x1

      # Refer to working copy flags.

        if hasfeature("execbit"):
            # Add 'x' flag to 'x1'
            $ hg debugimportstack << EOS | marks
            > [["commit", {"text": "J", "mark": ":10", "files": {"x1": {"data": "x1\n", "flags": "x", "copyFrom": "."}}, "parents": `marks :9`}],
            >  ["goto", {"mark": ":10"}]]
            > EOS
            {":10": "e78b6e74635f227cf2323f607a32a015c951d121"}

            # Reuse x1 flag from the working copy.
            $ hg debugimportstack << EOS | marks
            > [["commit", {"text": "K", "mark": ":11", "files": {"x1": {"data": "x2\n", "flags": "."}}, "parents": `marks :10`}],
            >  ["goto", {"mark": ":11"}]]
            > EOS
            {":11": "55a92b921ebb52f5f6c67896d61c83da3178bd92"}

            # Reuse x1 flag from the working copy parent, when x is deleted.
            $ rm x1
            $ hg debugimportstack << EOS | marks
            > [["commit", {"text": "L", "mark": ":12", "files": {"x1": {"data": "x2\n", "flags": "."}}, "parents": `marks :11`}],
            >  ["goto", {"mark": ":12"}]]
            > EOS
            {":12": "435dcd38184bec24509ee35a54b89ce1e8e3314e"}

            # Check the 'x' flag is present on x1 in the above commits.
            $ hg debugexportstack -r '.^+.' | pprint
            [{'author': 'test', 'date': [0.0, 0], 'immutable': False, 'node': 'e78b6e74635f227cf2323f607a32a015c951d121', 'relevantFiles': {'x1': {'data': 'x1\n', 'flags': 'x'}}, 'requested': False, 'text': 'J'},
             {'author': 'test',
              'date': [0.0, 0],
              'files': {'x1': {'data': 'x2\n', 'flags': 'x'}},
              'immutable': False,
              'node': '55a92b921ebb52f5f6c67896d61c83da3178bd92',
              'parents': ['e78b6e74635f227cf2323f607a32a015c951d121'],
              'requested': True,
              'text': 'K'},
             {'author': 'test',
              'date': [0.0, 0],
              'files': {},
              'immutable': False,
              'node': '435dcd38184bec24509ee35a54b89ce1e8e3314e',
              'parents': ['55a92b921ebb52f5f6c67896d61c83da3178bd92'],
              'requested': True,
              'text': 'L'}]
        else:
            # Still exercise the code paths to get coverage test pass, but do not test the result.
            $ touch x2
            $ rm x1
            $ hg debugimportstack > /dev/null << EOS
            > [["commit", {"text": "J1", "mark": ":10.1", "files": {"x1": {"data": "x1\n", "flags": ".", "copyFrom": "."}, "x2": {"data": "x2\n", "flags": "."}}, "parents": `marks :9`}]]
            > EOS

      # Write or delete files.
      # Deleted files will remove "A" status.
      # Written files will remove "R" status.

        $ newrepo repo-write --config format.use-eager-repo=True
        $ echo 1 > a
        $ echo 3 > c
        $ hg add c
        $ hg debugimportstack << EOS
        > [["write", {"a": {"data": "2\n"}, "b": {"dataBase85": "GYS"}, "c": null}]]
        > EOS
        {}
        $ f --dump a b c
        a:
        >>>
        2
        <<<
        b:
        >>>
        3
        <<<
        c: file not found
        $ hg st  # no "A c" or "! c"
        ? a
        ? b
        >>> assert "c" not in _
        $ hg commit -m 'Add a, b' -A a b

        $ hg rm a
        $ hg debugimportstack << EOS
        > [["write", {"a": {"data": "3\n"}}]]
        > EOS
        {}
        $ hg st  # no "R a"
        M a
        >>> assert "R a" not in _

      # Amend
      # Update Y to Y1, edit content of file Y to Y1, add new file P:

        $ newrepo
        $ drawdag << 'EOS'
        > X-Y  # Y/X=X1
        > EOS
        $ hg debugimportstack << EOS
        > [["amend", {"node": "$Y", "mark": ":1", "text": "Y1", "files": {"Y": {"data": "Y1"}, "P": {"data": "P"}}}]]
        > EOS
        {":1": "8567a23e6951126a1fc726a73324ee76ff5ed2cc"}
        $ hg log -Gr 'all()' -T '{desc}'
        o  Y1
        │
        o  X
        $ hg cat -r 'desc(Y)' P X Y
        PX1Y1 (no-eol)

      # Refer to working copy parent.
      # X is reverted from "XXX" to "X1"
      # Z is reverted from "Z" to "not found"

        $ hg up -q 'desc(Y)'
        $ echo XXX > X
        $ echo Z > Z
        $ hg debugimportstack << EOS
        > [["write", {"Z": ".", "X": "."}]]
        > EOS
        {}
        $ cat X Z
        cat: Z: $ENOENT$
        X1 (no-eol)
        [1]

      # Refer to existing file contents.

        $ newrepo
        $ drawdag << 'EOS'
        > X  # X/a.txt=content\n
        >    # X/c.txt=something\n
        > EOS
        $ hg debugimportstack << EOS
        > [["commit", {"author": "test1", "date": [3600, 3600], "text": "Y", "mark": ":1", "parents": ["$X"],
        >              "files": {"b.txt": {"dataRef": {"node": "$X", "path": "a.txt"}},
        >                        "c.txt": {"dataRef": {"node": "$X", "path": "missing"}}}}],
        >  ["goto", {"mark": ":1"}]]
        > EOS
        {":1": "*"} (glob)
        $ hg cat -r . b.txt
        content

      # Error cases.

        $ hg debugimportstack << EOS
        > [["foo", {}]]
        > EOS
        {"error": "unsupported action: ['foo', {}]"}
        [1]

        $ hg debugimportstack << EOS
        > not-json
        > EOS
        {"error": "commit info is invalid JSON (Expecting value: line 1 column 1 (char 0))"}
        [1]

        $ hg debugimportstack << EOS
        > [["commit", {"text": "x", "mark": "123"}]]
        > EOS
        {"error": "invalid mark: 123"}
        [1]

        $ hg debugimportstack << EOS
        > [["goto", {}]]
        > EOS
        {"error": "'mark'"}
        [1]


