#debugruntest-compatible
#chg-compatible

#require clang-format test-repo no-eden

  $ . "$TESTDIR/helpers-testrepo.sh"

  $ cd "$TESTDIR"/..
  $ for f in `testrepohg files mercurial | grep -E '\.(c|h)$' | grep -Ev -f contrib/clang-format-blacklist` ; do
  >   clang-format --style file "$f" > "$TESTTMP/formatted.txt"
  >   cmp "$f" "$TESTTMP/formatted.txt" || diff -u "$f" "$TESTTMP/formatted.txt"
  > done
