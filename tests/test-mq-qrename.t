
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

Test overlapping renames (issue2388)

  $ hg init c
  $ cd c
  $ hg qinit -c
  $ echo a > a
  $ hg add
  adding a
  $ hg qnew patcha
  $ echo b > b
  $ hg add
  adding b
  $ hg qnew patchb
  $ hg ci --mq -m c1
  $ hg qrename patchb patchc
  $ hg qrename patcha patchb
  $ hg st --mq
  M series
  A patchb
  A patchc
  R patcha
  $ cd ..

Test renames with mq repo (issue2097)

  $ hg init issue2097
  $ cd issue2097
  $ hg qnew p0
  $ (cd .hg/patches && hg init)
  $ hg qren p0 p1
  $ hg debugstate --mq
  $ hg ci --mq -mq0
  nothing changed
  [1]
  $ cd ..

Test renaming to a folded patch (issue3058)

  $ hg init issue3058
  $ cd issue3058
  $ hg init --mq
  $ echo a > a
  $ hg add a
  $ hg qnew adda
  $ echo b >> a
  $ hg qnew addb
  $ hg qpop
  popping addb
  now at: adda
  $ hg ci --mq -m "save mq"
  $ hg qfold addb
  $ hg qmv addb
  $ cat .hg/patches/addb
  # HG changeset patch
  # Parent 0000000000000000000000000000000000000000
  
  diff -r 000000000000 a
  --- /dev/null	* (glob)
  +++ b/a	* (glob)
  @@ -0,0 +1,2 @@
  +a
  +b
  $ cd ..

