#require test-repo pyflakes hg10

  $ . $TESTDIR/require-core-hg.sh tests/filterpyflakes.py

This file is backported from mercurial/tests/test-check-pyflakes.t.
It differs slightly to fix paths.

  $ . "$RUNTESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

run pyflakes on all tracked files ending in .py or without a file ending
(skipping binary file random-seed)

  $ hg locate 'set:**.py or grep("^#!.*python")' > "$TESTTMP/files1"
  $ if [ -n "$LINTFILES" ]; then
  >   printf "$LINTFILES" > "$TESTTMP/files2"
  >   join "$TESTTMP/files1" "$TESTTMP/files2"
  > else
  >   cat "$TESTTMP/files1"
  > fi \
  > | xargs pyflakes 2>/dev/null | "$RUNTESTDIR/filterpyflakes.py"
  
