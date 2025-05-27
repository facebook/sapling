#modern-config-incompatible

#require no-eden


  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"

  $ setupcommon

  $ enable commitcloud
  $ disable infinitepush
  $ hginit master
  $ cd master
  $ setupserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF
  $ echo x > x
  $ hg commit -qAm 'add x'
  $ hg bookmark master
  $ cd ..

Push a non-tree scratch branch from one client

  $ hgcloneshallow ssh://user@dummy/master normal-client -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
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
  $ hg push -q --to scratch/nontree --create
  $ clearcache
  $ cd ..

Push a tree-only scratch branch from another client
  $ hgcloneshallow ssh://user@dummy/master client1 -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
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
  $ hg push -q --to scratch/foo --create
  $ cd ..

Pull a non-tree scratch branch into a normal client

  $ hgcloneshallow ssh://user@dummy/master normal-client2 -q
  $ cd normal-client2
  $ hg pull -r scratch/nontree
  pulling from ssh://user@dummy/master
  searching for changes
  $ hg log -r tip -vp
  commit:      ebde88dba372
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
- Verify no new manifest revlog entry was written
- ...but we can still read the manifest
  $ hg log -r 02c12aef64ff --stat -T '{node}\n'
  02c12aef64ffa8bfcb6fe0054cb75084416dd43d
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
  $ hg log -r 02c12aef64ff  --stat
  commit:      02c12aef64ff
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add subdir/a
  
   subdir/a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  

Pull a treeonly scratch branch into a treeonly client (non-rebundling)

  $ hg pull -r scratch/foo
  pulling from ssh://user@dummy/master
  searching for changes
  $ hg log -G
  o  commit:      5a7a7de8a420
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     edit subdir/a
  │
  o  commit:      02c12aef64ff
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     add subdir/a
  │
  @  commit:      085784c01c08
     bookmark:    remote/master
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
  $ hg log -r 3ef288300b64 --stat
  commit:      3ef288300b64
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar/car
  
   bar/car |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
Pull a normal scratch branch into a treeonly client
  $ hg pull -r scratch/nontree
  pulling from ssh://user@dummy/master
  searching for changes
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
  commitcloud: head '7e75be1136c3' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  $ cd ..

Verify its not on the server
  $ cd master
  $ hg log -r 7e75be1136c3
  commit:      7e75be1136c3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo

Test delivering public and draft commits to the client. Verify we don't deliver
treemanifest data for the public commits.
  $ cd ../client1
  $ hg log -G -T '{node|short} {phase} {desc}'
  @  5a7a7de8a420 draft edit subdir/a
  │
  o  02c12aef64ff draft add subdir/a
  │
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

  $ hg log -G -T '{node|short} {phase} {desc}'
  o  02c12aef64ff draft add subdir/a
  │
  o  085784c01c08 public add x
  
# Verify only the infinitepush commit tree data was downloaded
# TODO(meyer): Replace packfile inspection with indexedlog inspection

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
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)
  $ hg merge d32fd17cb041
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -qm "merge"
  $ hg cloud backup
  commitcloud: head '8b1db7b72253' hasn't been uploaded yet
  edenapi: queue 2 commits for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
