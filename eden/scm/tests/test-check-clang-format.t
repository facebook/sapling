
#require clang-format version-control no-eden

  $ cd "$TESTDIR"/..
  warning: no longer inside TESTTMP
  $ blacklist='^(mercurial/cext/bdiff\.c|mercurial/cext/charencode\.c|mercurial/cext/charencode\.h|mercurial/cext/diffhelpers\.c|mercurial/cext/dirs\.c|mercurial/cext/manifest\.c|mercurial/cext/mpatch\.c|mercurial/cext/osutil\.c|mercurial/cext/revlog\.c)$'
  $ for f in `sl-source-files 'mercurial/**' | egrep '\.(c|h)$' | egrep -v "$blacklist"` ; do
  >   clang-format --style file "$f" > "$TESTTMP/formatted.txt"
  >   cmp "$f" "$TESTTMP/formatted.txt" || diff -u "$f" "$TESTTMP/formatted.txt"
  > done
