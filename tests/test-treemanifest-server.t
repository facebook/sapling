  $ . "$TESTDIR/library.sh"

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks=
  > pushrebase=
  > treemanifest=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > EOF
  $ mkdir subdir
  $ echo x > subdir/x
  $ hg commit -qAm 'add subdir/x'
  $ hg book mybook
  $ hg debugdata .hg/store/00manifesttree.i 0
  subdir\x00bc0c2c938b929f98b1c31a8c5994396ebb096bf0t (esc)
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master client -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ mkdir subdir2
  $ echo z >> subdir2/z
  $ hg commit -qAm "add subdir2/z"
  $ hg push --to mybook
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 1 changset:
  remote:     15486e46ccf6  add subdir2/z
  $ ls ../master/.hg/store/meta
  subdir
  subdir2
