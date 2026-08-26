
#require no-eden

  $ eagerepo
  $ enable amend
  $ setconfig commit.show-status=true commit.status-path-limit=4
  $ unset HGPLAIN
  $ unset HGPLAINEXCEPT

Commit shows the created commit and a bounded file summary:

  $ newrepo
  $ mkdir docs src tests
  $ echo docs > docs/readme
  $ echo a > src/a
  $ echo b > src/b
  $ echo c > src/c
  $ echo test > tests/test
  $ sl addremove -q
  $ sl commit -m initial
  committed * (glob)
  changed 5 file(s):
    docs/readme
    src/ (3 files)
    tests/test

Amend shows both commit hashes and only the files changed by the rewrite:

  $ echo changed >> src/a
  $ mkdir surprise
  $ echo surprise > surprise/file
  $ CODING_AGENT_METADATA=id=test_agent sl amend -A
  adding surprise/file
  amended * -> * (glob)
  changed 2 file(s):
    src/a
    surprise/file

Amend-to reports the rewritten target commit and amended paths:

  $ echo child > child
  $ HGPLAIN=1 sl commit -Aqm child
  $ echo changed-again >> docs/readme
  $ sl amend --to .^ docs/readme
  amended * -> * (glob)
  changed 1 file(s):
    docs/readme

Quiet suppresses the additional output:

  $ echo quiet > quiet
  $ sl add quiet
  $ sl commit -qm quiet
  $ echo changed >> quiet
  $ sl amend -q

Debug continues to show the committed hash when combined with quiet:

  $ echo debug > debug
  $ sl add debug
  $ sl commit --debug --quiet -m debug 2>&1 | $PYTHON -c 'import sys; sys.stdout.writelines(line for line in sys.stdin if line.startswith(("committed ", "changed ")))'
  committed * (glob)
  $ echo legacy-debug > legacy-debug
  $ sl add legacy-debug
  $ sl commit --debug --quiet --config commit.show-status=false -m legacy-debug 2>&1 | $PYTHON -c 'import sys; sys.stdout.writelines(line for line in sys.stdin if line.startswith(("committed ", "changed ")))'
  committed * (glob)

HGPLAIN suppresses the additional output:

  $ export HGPLAIN=1
  $ echo silent > silent
  $ sl add silent
  $ sl commit -m silent

HGPLAIN preserves the legacy debug commit line:

  $ echo silent-debug > silent-debug
  $ sl add silent-debug
  $ sl commit --debug --config commit.show-status=false -m silent-debug 2>&1 | $PYTHON -c 'import sys; sys.stdout.writelines(line for line in sys.stdin if line.startswith(("committed ", "changed ")))'
  committed * (glob)
