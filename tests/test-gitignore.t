  $ newrepo
  $ setconfig ui.gitignore=1

  $ cat > .gitignore << EOF
  > *.tmp
  > build/
  > EOF

  $ mkdir build exp
  $ cat > build/.gitignore << EOF
  > !*
  > EOF

  $ cat > exp/.gitignore << EOF
  > !i.tmp
  > EOF

  $ touch build/libfoo.so t.tmp Makefile exp/x.tmp exp/i.tmp

  $ hg status
  ? .gitignore
  ? Makefile
  ? exp/.gitignore
  ? exp/i.tmp
