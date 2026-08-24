  $ configure modern
  $ newclientrepo

  $ cat > "$TESTTMP/linter.py" <<'PY'
  > import os
  > import sys
  > from pathlib import Path
  > # Both working copy and staged tree runs execute from a project root
  > # holding the staged configuration file.
  > if not Path(".arcconfig").is_file():
  >     sys.exit(3)
  > with open(Path(os.environ["TESTTMP"]) / "lint-cwds", "a") as output:
  >     output.write(os.getcwd() + "\n")
  > replacements = {
  >     ("x.txt", "one"): "ONE",
  >     ("x.txt", "two"): "TWO",
  >     ("left.txt", "left"): "LEFT",
  >     ("right.txt", "right"): "RIGHT",
  >     ("redo.txt", "redo"): "REDO",
  >     ("s.txt", "three"): "THREE",
  >     ("batch1.txt", "b1"): "B1",
  >     ("batch2.txt", "b2"): "B2",
  >     ("count-a.txt", "a"): "A",
  >     ("count-b.txt", "b"): "B",
  > }
  > count = 0
  > for line in sys.stdin.buffer:
  >     path = Path(os.fsdecode(line.rstrip(b"\n")))
  >     count += 1
  >     logical_path = path.as_posix()
  >     content = path.read_bytes().splitlines()[0].decode()
  >     if content == "boom":
  >         sys.stderr.write("boom!\n")
  >         sys.exit(1)
  >     if content == "linked":
  >         # Prove linked entries resolve inside the project root.
  >         path.write_bytes(Path("linked-tools/marker.txt").read_bytes())
  >     elif (logical_path, content) in replacements:
  >         path.write_bytes((replacements[(logical_path, content)] + "\n").encode())
  >     elif logical_path.startswith("many-") and content == "many":
  >         path.write_bytes(b"MANY\n")
  > with open(Path(os.environ["TESTTMP"]) / "lint-calls", "a") as output:
  >     output.write(f"{count}\n")
  > PY
  $ cat >> "$HGRCPATH" <<EOF
  > [filelint]
  > linter.test.command = $PYTHON $TESTTMP/linter.py @-
  > linter.test.mode = staging-tree
  > linter.test.fix = true
  > linter.test.config-file.test = .arcconfig
  > max-file-size = 10MB
  > max-file-count = 100000
  > EOF

Build a stack with two heads where the tip edits a file introduced at the
bottom.

  $ sl debugdrawdag <<'EOS'
  > L R  # L/left.txt=left\n
  > |/   # R/right.txt=right\n
  > C    # C/x.txt=two\n
  > |
  > B    # B/y.txt=bee\n
  > |
  > A    # A/x.txt=one\n
  >      # A/ignored.bin=binary\n
  >      # A/.arcconfig={}\n
  >      # drawdag.defaultfiles=false
  > EOS
  $ sl goto -q C

Linting the stack stages every changed file version, including two
separate versions of x.txt (in separate trees, so the linter runs once
per tree) and both heads. The bottom result propagates through B, while C
gets its own result.

  $ sl lint
  running linters: test
  Found 4 "test" issues:
    left.txt
    right.txt
    x.txt (x2)
  warning: can't lint 1 linter configuration file(s)
  fixed 4 files and rewrote 5 commits
  $ sort "$TESTTMP/lint-calls"
  1
  5
  $ sl cat -r A x.txt
  ONE
  $ sl cat -r B x.txt
  ONE
  $ sl cat -r B y.txt
  bee
  $ sl cat -r C x.txt
  TWO
  $ sl cat -r L left.txt
  LEFT
  $ sl cat -r R right.txt
  RIGHT
  $ sl log -r . -T '{desc}\n'
  C
  $ cat x.txt
  TWO

A clean commit runs no linter at all because lint-clean content is remembered.

  $ wc -l < "$TESTTMP/lint-calls"
  2
  $ sl lint -r .
  nothing changed
  $ wc -l < "$TESTTMP/lint-calls"
  2

The --quiet flag hides linter and fix output.

  $ printf 'redo\n' > redo.txt
  $ sl add redo.txt
  $ sl commit -qm REDO
  $ sl amend -m REDO2
  $ sl lint --quiet -r .
  $ sl cat -r . redo.txt
  REDO
  $ sl log -r . -T '{desc}\n'
  REDO2

Disabling the cache reruns the linters over clean content without rewriting.

  $ sl lint -r .
  nothing changed
  $ wc -l < "$TESTTMP/lint-calls"
  3
  $ sl --config filelint.cache=false lint --verbose -r .
  running linters: test
  Found 0 "test" issues
  nothing changed
  $ wc -l < "$TESTTMP/lint-calls"
  4

An ADVANCED flag clears recorded content so the linters run again.

  $ sl lint --clear-cache -r .
  running linters: test
  nothing changed
  $ wc -l < "$TESTTMP/lint-calls"
  5

Linting only an ancestor restacks descendants without touching their own
content: S2's edit of s.txt stays unfixed because S2 was not selected.

  $ sl goto -q A
  $ printf 'three\n' > s.txt
  $ sl add s.txt
  $ sl commit -qm S1
  $ printf 'four\n' > s.txt
  $ sl commit -qm S2
  $ sl lint -r 'desc(S1)'
  running linters: test
  Found 1 "test" issue:
    s.txt
  fixed 1 files and rewrote 2 commits
  $ sl cat -r 'desc(S1)' s.txt
  THREE
  $ sl cat -r 'desc(S2)' s.txt
  four

A linter failure aborts without rewriting anything and cleans up the
staging tree.

  $ sl goto -q A
  $ printf 'boom\n' > boom.txt
  $ sl add boom.txt
  $ sl commit -qm BOOM
  $ mkdir "$TESTTMP/stage-tmp"
  $ TMPDIR=$TESTTMP/stage-tmp sl lint -r .
  running linters: test
  abort: linter test failed: boom!
  [255]
  $ sl cat -r . boom.txt
  boom

The linter ran from a staged tree outside the repository, and the tree is
removed even when linting fails.

  $ tail -1 "$TESTTMP/lint-cwds" | grep -q stage-tmp && echo staged-outside-repo
  staged-outside-repo
  $ ls "$TESTTMP/stage-tmp"

Staged files are split into bounded per-command batches.

  $ sl goto -q A
  $ printf 'b1\n' > batch1.txt
  $ printf 'b2\n' > batch2.txt
  $ sl add batch1.txt batch2.txt
  $ sl commit -qm batches
  $ sl --config filelint.max-files-per-command=1 lint -r .
  running linters: test
  Found 2 "test" issues:
    batch1.txt
    batch2.txt
  fixed 2 files and rewrote 1 commits
  $ tail -2 "$TESTTMP/lint-calls"
  1
  1

With --no-fix, needed fixes are reported without rewriting anything or
recording content as clean.

  $ sl goto -q A
  $ printf 'redo\n' > redo.txt
  $ sl add redo.txt
  $ sl commit -qm NOFIX
  $ sl lint --no-fix -r .
  running linters: test
  Found 1 "test" issue:
    redo.txt
  1 file(s) need fixes
  [1]
  $ sl cat -r . redo.txt
  redo
  $ sl log -r . -T '{desc}\n'
  NOFIX
  $ sl lint -r .
  running linters: test
  Found 1 "test" issue:
    redo.txt
  fixed 1 files and rewrote 1 commits
  $ sl cat -r . redo.txt
  REDO

Long fix lists are truncated.

  $ sl goto -q A
  $ printf 'many\n' > many-1.txt
  $ printf 'many\n' > many-2.txt
  $ printf 'many\n' > many-3.txt
  $ printf 'many\n' > many-4.txt
  $ printf 'many\n' > many-5.txt
  $ printf 'many\n' > many-6.txt
  $ printf 'many\n' > many-7.txt
  $ sl add many-1.txt many-2.txt many-3.txt many-4.txt many-5.txt many-6.txt many-7.txt
  $ sl commit -qm MANY
  $ sl lint -r .
  running linters: test
  Found 7 "test" issues:
    many-1.txt
    many-2.txt
    many-3.txt
    many-4.txt
    many-5.txt
    ... and 2 more
  fixed 7 files and rewrote 1 commits

Linter configuration files are skipped with a warning.

  $ sl goto -q C
  $ printf '{"x":1}\n' > .arcconfig
  $ sl commit -qm config
  $ sl lint -r .
  warning: can't lint 1 linter configuration file(s)
  nothing changed
  $ sl cat -r . .arcconfig
  {"x":1}

Symlinks are skipped.

#if symlink
  $ ln -s x.txt link.txt
  $ sl add link.txt
  $ sl commit -qm symlink
  $ sl lint -r .
  nothing changed
#endif

Configured working-copy entries are linked into each staged tree, making
it a self-contained project root; files under the links are not staged
(the linter would rewrite the working copy through them), so they are
skipped with a warning.

#if symlink
  $ mkdir linked-tools
  $ printf 'MARKER\n' > linked-tools/marker.txt
  $ printf 'inner\n' > linked-tools/inner.txt
  $ printf 'linked\n' > uses-link.txt
  $ sl add linked-tools/marker.txt linked-tools/inner.txt uses-link.txt
  $ sl commit -qm linked
  $ sl --config 'filelint.linter.test.staging-symlinks=linked-tools' lint -r .
  running linters: test
  Found 1 "test" issue:
    uses-link.txt
  warning: can't lint 2 file(s) under linter staging-symlinks paths
  fixed 1 files and rewrote 1 commits
  $ sl cat -r . uses-link.txt
  MARKER
  $ sl cat -r . linked-tools/inner.txt
  inner
#endif

Oversized content is skipped without fetching it.

  $ printf 'this file is too large\n' > large.txt
  $ sl add large.txt
  $ sl commit -qm large
  $ sl --config filelint.max-file-size=10 lint -r .
  warning: can't lint 1 file(s) larger than 10 bytes
  nothing changed

Files over the count limit are skipped while the remaining files are
linted.

  $ printf 'a\n' > count-a.txt
  $ printf 'b\n' > count-b.txt
  $ sl add count-a.txt count-b.txt
  $ sl commit -qm count
  $ sl --config filelint.max-file-count=1 lint -r .
  running linters: test
  Found 1 "test" issue:
    count-a.txt
  warning: can't lint 1 file(s) because filelint.max-file-count is 1
  fixed 1 files and rewrote 1 commits
  $ cat count-a.txt
  A
  $ cat count-b.txt
  b

Merge commits are rejected before linting or replay begins.

  $ sl merge -q L
  $ sl commit -qm M
  $ sl lint -r .
  abort: cannot lint merge commits
  [255]
