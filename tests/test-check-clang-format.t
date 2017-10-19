#require clang-format test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"

  $ cd "$TESTDIR"/..
  $ for f in `testrepohg files 'set:(**.c or **.h) and not "listfile:contrib/clang-format-blacklist"'` ; do
  >   clang-format --style file $f > $f.formatted
  >   cmp $f $f.formatted || diff -u $f $f.formatted
  >   rm $f.formatted
  > done
