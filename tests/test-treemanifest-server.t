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

Test that local commits on the server produce trees
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
  $ mkdir subdir2
  $ echo z >> subdir2/z
  $ hg commit -qAm "add subdir2/z"

Test pushing without pushrebase fails

  $ hg push
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: transaction abort!
  remote: rollback completed
  remote: cannot push commits to a treemanifest transition server without pushrebase
  abort: push failed on remote
  [255]

Test pushing with pushrebase creates trees on the server
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ hg push --to mybook
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 1 changset:
  remote:     15486e46ccf6  add subdir2/z
  $ ls ../master/.hg/store/meta
  subdir
  subdir2
  $ cd ../master
  $ hg debugdata .hg/store/00manifest.i 1
  subdir/x\x001406e74118627694268417491f018a4a883152f0 (esc)
  subdir2/z\x0069a1b67522704ec122181c0890bd16e9d3e7516a (esc)
  $ hg debugdata .hg/store/00manifesttree.i 1
  subdir\x00bc0c2c938b929f98b1c31a8c5994396ebb096bf0t (esc)
  subdir2\x00ddb35f099a648a43a997aef53123bce309c794fdt (esc)
