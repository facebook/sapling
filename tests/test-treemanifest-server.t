  $ . "$TESTDIR/library.sh"

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks=$TESTDIR/../hgext3rd/bundle2hooks.py
  > pushrebase=$TESTDIR/../hgext3rd/pushrebase.py
  > treemanifest=$TESTDIR/../treemanifest
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
  > pushrebase=$TESTDIR/../hgext3rd/pushrebase.py
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

Test stripping trees
  $ hg up -q tip
  $ echo a >> subdir/a
  $ hg commit -Aqm 'modify subdir/a'
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      50     -1       0 85b359fdb09e 000000000000 000000000000
       1        50      62      0       1 54cbf534b62b 85b359fdb09e 000000000000
       2       112      61      1       2 a6f4164c3e4e 54cbf534b62b 000000000000
  $ hg debugindex .hg/store/meta/subdir/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      54      0       2 126c4ddee02e bc0c2c938b92 000000000000
  $ hg strip -r tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/0619d7982079-bc05b04f-backup.hg (glob)
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      50     -1       0 85b359fdb09e 000000000000 000000000000
       1        50      62      0       1 54cbf534b62b 85b359fdb09e 000000000000
  $ hg debugindex .hg/store/meta/subdir/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
