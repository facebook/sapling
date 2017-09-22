  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH
  $ setupcommon

  $ hginit master
  $ cd master
  $ setupserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=$TESTDIR/../hgext3rd/pushrebase.py
  > treemanifest=$TESTDIR/../treemanifest
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm 'add x'
  $ cd ..

Push a scratch branch from one client

  $ hgcloneshallow ssh://user@dummy/master client1 -q --config extensions.treemanifest=$TESTDIR/../treemanifest --config treemanifest.treeonly=True
  1 trees fetched over * (glob)
  1 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client1
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../treemanifest
  > 
  > [remotefilelog]
  > usefastdatapack=True
  > 
  > [treemanifest]
  > treeonly=True
  > EOF

  $ mkdir subdir
  $ echo "my change" >> subdir/a
  $ hg commit -qAm 'add subdir/a'
  $ hg push --to scratch/foo --create
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 1 commit:
  remote:     02c12aef64ff  add subdir/a
  $ cd ..

Pull a scratch branch from another client

  $ hgcloneshallow ssh://user@dummy/master client2 -q --config extensions.treemanifest=$TESTDIR/../treemanifest --config treemanifest.treeonly=True
  $ cd client2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../treemanifest
  > 
  > [remotefilelog]
  > usefastdatapack=True
  > 
  > [treemanifest]
  > treeonly=True
  > EOF
  $ hg pull -r scratch/foo
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg log -G
  o  changeset:   1:02c12aef64ff
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add subdir/a
  |
  @  changeset:   0:085784c01c08
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add x
  
  $ hg cat -r tip subdir/a
  my change
  $ ls_l .hg/store
  -rw-r--r--     257 00changelog.i
  -rw-r--r--     108 00manifesttree.i
  drwxr-xr-x         data
  drwxrwxr-x         packs
  -rw-r--r--      43 phaseroots
  -rw-r--r--      18 undo
  -rw-r--r--      17 undo.backupfiles
  -rw-r--r--       0 undo.phaseroots
  $ cd ..

Verify its not on the server
  $ cd master
  $ hg log -G
  @  changeset:   0:085784c01c08
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add x
  
