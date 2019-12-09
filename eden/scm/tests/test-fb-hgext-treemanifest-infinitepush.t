#chg-compatible

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setconfig treemanifest.flatcompat=False

  $ setupcommon

  $ hginit master
  $ cd master
  $ setupserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > [treemanifest]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm 'add x'
  $ cd ..

Push a non-tree scratch branch from one client

  $ hgcloneshallow ssh://user@dummy/master normal-client -q
  fetching tree '' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd normal-client
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > sendtrees=True
  > EOF
  $ mkdir bar
  $ echo >> bar/car
  $ hg commit -qAm 'add bar/car'
  $ echo >> bar/car
  $ hg commit -qm 'edit bar/car'
  $ hg push --to scratch/nontree --create
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 2 commits:
  remote:     3ef288300b64  add bar/car
  remote:     ebde88dba372  edit bar/car
  $ clearcache
  $ cd ..

Push a tree-only scratch branch from another client
  $ hgcloneshallow ssh://user@dummy/master client1 -q
  fetching tree '' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client1
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > sendtrees=True
  > EOF

  $ mkdir subdir
  $ echo "my change" >> subdir/a
  $ hg commit -qAm 'add subdir/a'
  $ echo "my other change" >> subdir/a
  $ hg commit -qAm 'edit subdir/a'
  $ hg push --to scratch/foo --create
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 2 commits:
  remote:     02c12aef64ff  add subdir/a
  remote:     5a7a7de8a420  edit subdir/a
  $ cd ..

Pull a non-tree scratch branch into a normal client

  $ hgcloneshallow ssh://user@dummy/master normal-client2 -q
  $ cd normal-client2
  $ hg pull -r scratch/nontree
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  new changesets 3ef288300b64:ebde88dba372
  $ hg log -r tip -vp
  changeset:   2:ebde88dba372
  bookmark:    scratch/nontree
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/car
  description:
  edit bar/car
  
  
  diff -r 3ef288300b64 -r ebde88dba372 bar/car
  --- a/bar/car	Thu Jan 01 00:00:00 1970 +0000
  +++ b/bar/car	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   
  +
  
Pull a treeonly scratch branch into a normal client
  $ hg pull -r scratch/foo
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  new changesets 02c12aef64ff:5a7a7de8a420
- Verify no new manifest revlog entry was written
- ...but we can still read the manifest
  $ hg log -r 02c12aef64ff --stat -T '{rev}\n'
  3
   subdir/a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ cd ..

Set up another treeonly client

  $ hgcloneshallow ssh://user@dummy/master client2 -q
  $ cd client2

Pull just part of a treeonly scratch branch (this causes rebundling on the server)

  $ hg pull -r 02c12aef64ff
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 02c12aef64ff
  $ hg log -r 02c12aef64ff  --stat
  changeset:   1:02c12aef64ff
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add subdir/a
  
   subdir/a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  

Pull a treeonly scratch branch into a treeonly client (non-rebundling)

  $ hg pull -r scratch/foo
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 1 files
  new changesets 5a7a7de8a420
  $ hg log -G
  o  changeset:   2:5a7a7de8a420
  |  bookmark:    scratch/foo
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     edit subdir/a
  |
  o  changeset:   1:02c12aef64ff
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
  my other change

Pull just part of a normal scratch branch (this causes rebundling on the server)

  $ hg pull -r 3ef288300b64
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 3ef288300b64
  $ hg log -r 3ef288300b64 --stat
  changeset:   3:3ef288300b64
  tag:         tip
  parent:      0:085784c01c08
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar/car
  
   bar/car |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
Pull a normal scratch branch into a treeonly client
  $ hg pull -r scratch/nontree
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 1 files
  new changesets ebde88dba372
  $ hg log -r 3ef288300b64 -T ' ' --stat
    bar/car |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg log -r 3ef288300b64 -T ' ' --stat
    bar/car |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ cd ..

Verify hg cloud backup in a treeonly client will convert old flat manifests into
trees
  $ hgcloneshallow ssh://user@dummy/master ondemandconvertclient -q
  $ cd ondemandconvertclient
  $ echo >> foo
  $ hg commit -Aqm 'add foo'
  $ hg up -q '.^'
  $ hg cloud backup
  backing up stack rooted at 7e75be1136c3
  remote: pushing 1 commit:
  remote:     7e75be1136c3  add foo
  commitcloud: backed up 1 commit
  $ cd ..

Verify its not on the server
  $ cd master
  $ hg log -G
  @  changeset:   0:085784c01c08
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add x
  
Test delivering public and draft commits to the client. Verify we don't deliver
treemanifest data for the public commits.
  $ cd ../client1
  $ hg log -G -T '{node|short} {phase} {desc}'
  @  5a7a7de8a420 draft edit subdir/a
  |
  o  02c12aef64ff draft add subdir/a
  |
  o  085784c01c08 public add x
  
# Strip all the commits so we can pull them again.
  $ hg debugstrip -q -r 'all()' --no-backup

# Clear out all the tree data, so we can see exactly what is downloaded in the
# upcoming pull.
  $ rm -rf .hg/store/packs/*
  $ clearcache

# Pull one infinitepush commit and one normal commit
  $ hg pull -r 02c12aef64ffa8bfc
  pulling from ssh://user@dummy/master
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 085784c01c08:02c12aef64ff
  1 trees fetched over * (glob)

  $ hg log -G -T '{node|short} {phase} {desc}'
  o  02c12aef64ff draft add subdir/a
  |
  o  085784c01c08 public add x
  
# Verify only the infinitepush commit tree data was downloaded
  $ hg debugdatapack .hg/store/packs/manifests/*datapack
  .hg/store/packs/manifests/a9b899bcf54bca96b39e9e135ca0625126487ceb:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  9eee655b90d1  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  604088751312  000000000000  92            (missing)
  

# Create a new commit on master with a noticeable number of trees
  $ cd ../master
  $ mkdir -p deep/dir/for/many/trees
  $ echo x > deep/dir/for/many/trees/x
  $ hg commit -Aqm "add deep x"
  $ cd ../client1
  $ hg pull -q

# Create a new root with just one tree
  $ hg up -q null
  $ echo z > z
  $ hg commit -Aqm "add z"

# Merge the root into master and push the merge as a backup
  $ hg up -q f027ebc7ba78
  fetching tree '' 92ea8e774335a78205d4837583cf4224b5fc5c33, based on bc0c2c938b929f98b1c31a8c5994396ebb096bf0, found via f027ebc7ba78
  6 trees fetched over * (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)
  $ hg merge d32fd17cb041
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -qm "merge"
  $ hg cloud backup
  backing up stack rooted at d32fd17cb041
  remote: pushing 2 commits:
  remote:     d32fd17cb041  add z
  remote:     8b1db7b72253  merge
  commitcloud: backed up 2 commits

# Check the bundle.  It should only have 2 trees (one from z and one for the merged
# root directory)
  $ hg debugbundle $TESTTMP/master/.hg/scratchbranches/filebundlestore/95/ac/95ac1702067611e314fd8e7d61ed1ff6d2485228
  Stream params: {}
  changegroup -- {version: 02}
      d32fd17cb041b810cad28724776c6d51faad59dc
      8b1db7b722533971a8133917e17a356a729cc281
  b2x:treegroup2 -- {cache: False, category: manifests, version: 1}
      2 data items, 2 history items
