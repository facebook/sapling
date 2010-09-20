
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init a
  $ cd a

  $ echo 'base' > base
  $ hg ci -Ambase
  adding base

  $ hg qnew -mmqbase mqbase

  $ hg qrename mqbase renamed
  $ mkdir .hg/patches/foo
  $ hg qrename renamed foo

  $ hg qseries
  foo/renamed

  $ ls .hg/patches/foo
  renamed

  $ mkdir .hg/patches/bar
  $ hg qrename foo/renamed bar

  $ hg qseries
  bar/renamed

  $ ls .hg/patches/bar
  renamed

  $ hg qrename bar/renamed baz

  $ hg qseries
  baz

  $ ls .hg/patches/baz
  .hg/patches/baz

  $ hg qrename baz new/dir

  $ hg qseries
  new/dir

  $ ls .hg/patches/new/dir
  .hg/patches/new/dir

  $ cd ..

Test patch being renamed before committed:

  $ hg init b
  $ cd b
  $ hg qinit -c
  $ hg qnew x
  $ hg qrename y
  $ hg qcommit -m rename

  $ cd ..


