  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"

  $ setupcommon

  $ hginit master
  $ cd master
  $ setupserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > treemanifest=
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm 'add x'
  $ cd ..

Push a non-tree scratch branch from one client

  $ hgcloneshallow ssh://user@dummy/master normal-client -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd normal-client
  $ mkdir bar
  $ echo >> bar/car
  $ hg commit -qAm 'add bar/car'
  $ echo >> bar/car
  $ hg commit -qm 'edit bar/car'
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > fastmanifest=
  > 
  > [remotefilelog]
  > usefastdatapack=True
  > 
  > [fastmanifest]
  > usecache=False
  > usetree=True
  > EOF
  $ hg push --to scratch/nontree --create
  pushing to ssh://user@dummy/master
  searching for changes
  1 trees fetched over * (glob)
  remote: pushing 2 commits:
  remote:     42ec76eb772a  add bar/car
  remote:     6a9819ced061  edit bar/car
  $ clearcache
  $ cd ..

Push a tree-only scratch branch from another client
  $ hgcloneshallow ssh://user@dummy/master client1 -q --config extensions.treemanifest= --config treemanifest.treeonly=True
  1 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client1
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > 
  > [remotefilelog]
  > usefastdatapack=True
  > 
  > [treemanifest]
  > treeonly=True
  > sendtrees=True
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

Pull a non-tree scratch branch into a normal client

  $ hgcloneshallow ssh://user@dummy/master normal-client2 -q
  $ cd normal-client2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > fastmanifest=
  > 
  > [remotefilelog]
  > usefastdatapack=True
  > 
  > [fastmanifest]
  > usecache=False
  > usetree=True
  > EOF
  $ hg pull -r scratch/nontree
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  new changesets 42ec76eb772a:6a9819ced061
  (run 'hg update' to get a working copy)
  $ hg log -r tip -vp
  changeset:   2:6a9819ced061
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/car
  description:
  edit bar/car
  
  
  diff -r 42ec76eb772a -r 6a9819ced061 bar/car
  --- a/bar/car	Thu Jan 01 00:00:00 1970 +0000
  +++ b/bar/car	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   
  +
  
Pull a treeonly scratch branch into a normal client
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      59      0       1 bf0601d5cb94 bc0c2c938b92 000000000000
       2       103      61      1       2 2e51d102996d bf0601d5cb94 000000000000
  $ hg pull -r scratch/foo
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 02c12aef64ff
  (run 'hg heads' to see heads, 'hg merge' to merge)
- Verify no new manifest revlog entry was written
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      59      0       1 bf0601d5cb94 bc0c2c938b92 000000000000
       2       103      61      1       2 2e51d102996d bf0601d5cb94 000000000000
- ...but we can still read the manifest
  $ hg log -r 02c12aef64ff --stat -T '{rev}\n'
  3
   subdir/a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ cd ..

Pull a treeonly scratch branch into a treeonly client

  $ hgcloneshallow ssh://user@dummy/master client2 -q --config extensions.treemanifest= --config treemanifest.treeonly=True
  $ cd client2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
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
  new changesets 02c12aef64ff
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

Pull just part of a normal scratch branch (this causes rebundling on the server)

  $ hg pull -r 42ec76eb772a
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 42ec76eb772a
  (run 'hg heads' to see heads, 'hg merge' to merge)

Pull a normal scratch branch into a treeonly client
  $ hg pull -r scratch/nontree
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 1 files
  new changesets 6a9819ced061
  (run 'hg update' to get a working copy)
  $ hg log -r 42ec76eb772a -T ' ' --stat
  abort: "unable to find the following nodes locally or on the server: ('', bf0601d5cb94247e00d0bdd1d8327f0dd36f54e9)"
  [255]
  $ hg log -r 42ec76eb772a -T ' ' --stat
  abort: "unable to find the following nodes locally or on the server: ('', bf0601d5cb94247e00d0bdd1d8327f0dd36f54e9)"
  [255]
  $ cd ..

Verify its not on the server
  $ cd master
  $ hg log -G
  @  changeset:   0:085784c01c08
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add x
  
