Failed qimport of patches from files should cleanup by recording successfully
imported patches in series file.

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg ci -Am'add a'
  adding a
  $ cat >b.patch<<EOF
  > diff --git a/a b/a
  > --- a/a
  > +++ b/a
  > @@ -1,1 +1,2 @@
  >  a
  > +b
  > EOF
  $ echo
  

empty series

  $ hg qseries
  $ echo
  

qimport valid patch followed by invalid patch

  $ hg qimport b.patch fakepatch
  adding b.patch to series file
  abort: unable to read file fakepatch
  [255]
  $ echo
  

valid patches before fail added to series

  $ hg qseries
  b.patch
