#require clang-format test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"

  $ cd "$TESTDIR"/..
  $ for f in `testrepohg files mercurial | egrep '\.(c|h)$' | egrep -v -f contrib/clang-format-blacklist` ; do
  >   clang-format --style file "$f" > "$TESTTMP/formatted.txt"
  >   cmp "$f" "$TESTTMP/formatted.txt" || diff -u "$f" "$TESTTMP/formatted.txt"
  > done
