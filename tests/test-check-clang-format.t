#require clang-format test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"

  $ cd "$TESTDIR"/..
  $ for f in `testrepohg files 'set:(mercurial/**.c or mercurial/**.h) and not "listfile:contrib/clang-format-blacklist"'` ; do
  >   clang-format --style file "$f" > "$TESTTMP/formatted.txt"
  >   cmp "$f" "$TESTTMP/formatted.txt" || diff -u "$f" "$TESTTMP/formatted.txt"
  > done
