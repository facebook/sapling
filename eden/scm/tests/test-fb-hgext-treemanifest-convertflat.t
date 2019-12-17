#chg-compatible

  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH <<EOF
  > [treemanifest]
  > sendtrees=True
  > EOF

Setup the server

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

Make local commits on the server
  $ mkdir subdir
  $ echo x > subdir/x
  $ hg commit -qAm 'add subdir/x'

Verify server commits produce correct trees during the conversion
  $ echo tomodify > subdir/tomodify
  $ echo toremove > subdir/toremove
  $ echo tomove > subdir/tomove
  $ echo tocopy > subdir/tocopy
  $ hg commit -qAm 'create files'
  $ echo >> subdir/tomodify
  $ hg rm subdir/toremove
  $ hg mv subdir/tomove subdir/tomove2
  $ hg cp subdir/tocopy subdir/tocopy2
  $ hg commit -qAm 'remove, move, copy'
  $ hg status --change . -C
  M subdir/tomodify
  A subdir/tocopy2
    subdir/tocopy
  A subdir/tomove2
    subdir/tomove
  R subdir/tomove
  R subdir/toremove
  $ hg status --change . -C
  M subdir/tomodify
  A subdir/tocopy2
    subdir/tocopy
  A subdir/tomove2
    subdir/tomove
  R subdir/tomove
  R subdir/toremove
  $ hg debugstrip -r '.^' --no-backup
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved

The following will simulate the transition from flat to tree-only
1. Flat only client, with flat only draft commits
2. Hybrid client, with some flat and some flat+tree draft commits
3. Tree-only client, with only tree commits (old flat are converted)

Create flat manifest client
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master client -q
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > amend=
  > pushrebase=
  > EOF

Make a flat-only draft commit tree
  $ echo f1 >> subdir/x
  $ hg commit -qm 'flat only commit 1 at level 1'
  $ echo f11 >> subdir/x
  $ hg commit -qm 'flat only commit 1 over flat only commit 1 at level 1'
  $ hg up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo f12 >> subdir/x
  $ hg commit -qm 'flat only commit 2 over flat only commit 1 at level 1'
  $ echo f121 >> subdir/x
  $ hg commit -qm 'flat only commit 1 over flat only commit 2 at level 2'
  $ hg up '.^^^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Transition to treeonly client
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > demanddownload=True
  > EOF

Test working with flat-only draft commits.

- There are no local tree packs.
  $ ls_l .hg/store/packs | grep manifests
  drwxrwxr-x         manifests

- Viewing flat draft commit would fail when 'treemanifest.demandgenerate' is
False in treeonly mode because there is no tree manifest.

  $ hg log -vpr 'b9b574be2f5d' --config treemanifest.demandgenerate=False \
  > 2>&1 > /dev/null | tail -1

- Viewing a flat draft commit in treeonly mode will generate a tree manifest
for all the commits in the path from the flat draft commit to an ancestor which
has tree manifest. In this case, this implies that tree manifest will be
generated for the commit 'b9b574be2f5d' and its parent commit '9055b56f3916'.

  $ hg log -vpr 'b9b574be2f5d'
  changeset:   2:b9b574be2f5d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       subdir/x
  description:
  flat only commit 1 over flat only commit 1 at level 1
  
  
  diff -r 9055b56f3916 -r b9b574be2f5d subdir/x
  --- a/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   x
   f1
  +f11
  
- Now that we have the tree manifest for commit 'b9b574be2f5d', we should be
able to view it even with 'treemanifest.demandgenerate' being False.

  $ hg log -vpr 'b9b574be2f5d' --config treemanifest.demandgenerate=False
  changeset:   2:b9b574be2f5d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       subdir/x
  description:
  flat only commit 1 over flat only commit 1 at level 1
  
  
  diff -r 9055b56f3916 -r b9b574be2f5d subdir/x
  --- a/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   x
   f1
  +f11
  
- We should be able to also view the parent of commit 'b9b574be2f5d' i.e. commit
'9055b56f3916' because we now have the tree manifest for it.

  $ hg log -vpr '9055b56f3916' --config treemanifest.demandgenerate=False
  changeset:   1:9055b56f3916
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       subdir/x
  description:
  flat only commit 1 at level 1
  
  
  diff -r 2278cc8c6ce6 -r 9055b56f3916 subdir/x
  --- a/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   x
  +f1
  
- Check the tree manifest for commit '9055b56f3916' and 'b9b574be2f5d'.

  $ ls_l .hg/store/packs/manifests
  -r--r--r--    1196 4efbca00685bceff6359c358de842938789d6d3a.histidx
  -r--r--r--     183 4efbca00685bceff6359c358de842938789d6d3a.histpack
  -r--r--r--    1196 574eeaafb26148d853004c1617e6f8f11c743709.histidx
  -r--r--r--     183 574eeaafb26148d853004c1617e6f8f11c743709.histpack
  -r--r--r--    1114 5e71d43af637c17fb8ad3b3eb9799f9ea30fa786.dataidx
  -r--r--r--     219 5e71d43af637c17fb8ad3b3eb9799f9ea30fa786.datapack
  -r--r--r--    1114 61406e4caf3e020d101d44b3a0790ad31ac67e05.dataidx
  -r--r--r--     219 61406e4caf3e020d101d44b3a0790ad31ac67e05.datapack
  -r--r--r--    1114 7a828dc4ddbfaeff244f0e49c9a57ae4b90b8d2e.dataidx
  -r--r--r--     219 7a828dc4ddbfaeff244f0e49c9a57ae4b90b8d2e.datapack
  -r--r--r--    1196 8aa73f0a2fee603010f1e55513a484b15ab84a9f.histidx
  -r--r--r--     183 8aa73f0a2fee603010f1e55513a484b15ab84a9f.histpack
  -r--r--r--    1114 eef786b4f59e9cc9a04bd2ac252a9f9b72bcca70.dataidx
  -r--r--r--     219 eef786b4f59e9cc9a04bd2ac252a9f9b72bcca70.datapack
  -r--r--r--    1196 f2f83026385a0ae7128583b50734e5d09f0b66ec.histidx
  -r--r--r--     183 f2f83026385a0ae7128583b50734e5d09f0b66ec.histpack

- Tree manifest data for commit '9055b56f3916'.

  $ hg debugdatapack .hg/store/packs/manifests/*.datapack
  .hg/store/packs/manifests/5e71d43af637c17fb8ad3b3eb9799f9ea30fa786:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  397e59856f06  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  53c631458e33  000000000000  49            (missing)
  
  .hg/store/packs/manifests/61406e4caf3e020d101d44b3a0790ad31ac67e05:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  906f17f69284  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  a6875e5fbf69  000000000000  49            (missing)
  
  .hg/store/packs/manifests/7a828dc4ddbfaeff244f0e49c9a57ae4b90b8d2e:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  33600a12f793  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  40f43426c87b  000000000000  49            (missing)
  
  .hg/store/packs/manifests/eef786b4f59e9cc9a04bd2ac252a9f9b72bcca70:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  31a1621c0fb2  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  b7db2b1fa98f  000000000000  49            (missing)
  

- Again, this would generate the tree manifest from the corresponding flat
manifest for commit 'f7febcf0f689'.

  $ hg log -vpr 'f7febcf0f689'
  changeset:   3:f7febcf0f689
  parent:      1:9055b56f3916
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       subdir/x
  description:
  flat only commit 2 over flat only commit 1 at level 1
  
  
  diff -r 9055b56f3916 -r f7febcf0f689 subdir/x
  --- a/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   x
   f1
  +f12
  
  $ ls_l .hg/store/packs/manifests
  -r--r--r--    1196 4efbca00685bceff6359c358de842938789d6d3a.histidx
  -r--r--r--     183 4efbca00685bceff6359c358de842938789d6d3a.histpack
  -r--r--r--    1196 574eeaafb26148d853004c1617e6f8f11c743709.histidx
  -r--r--r--     183 574eeaafb26148d853004c1617e6f8f11c743709.histpack
  -r--r--r--    1114 5e71d43af637c17fb8ad3b3eb9799f9ea30fa786.dataidx
  -r--r--r--     219 5e71d43af637c17fb8ad3b3eb9799f9ea30fa786.datapack
  -r--r--r--    1114 61406e4caf3e020d101d44b3a0790ad31ac67e05.dataidx
  -r--r--r--     219 61406e4caf3e020d101d44b3a0790ad31ac67e05.datapack
  -r--r--r--    1114 7a828dc4ddbfaeff244f0e49c9a57ae4b90b8d2e.dataidx
  -r--r--r--     219 7a828dc4ddbfaeff244f0e49c9a57ae4b90b8d2e.datapack
  -r--r--r--    1196 8aa73f0a2fee603010f1e55513a484b15ab84a9f.histidx
  -r--r--r--     183 8aa73f0a2fee603010f1e55513a484b15ab84a9f.histpack
  -r--r--r--    1114 eef786b4f59e9cc9a04bd2ac252a9f9b72bcca70.dataidx
  -r--r--r--     219 eef786b4f59e9cc9a04bd2ac252a9f9b72bcca70.datapack
  -r--r--r--    1196 f2f83026385a0ae7128583b50734e5d09f0b66ec.histidx
  -r--r--r--     183 f2f83026385a0ae7128583b50734e5d09f0b66ec.histpack

- Tree manifest data for commit 'f7febcf0f689'.

  $ hg debugdatapack .hg/store/packs/manifests/*.datapack
  .hg/store/packs/manifests/5e71d43af637c17fb8ad3b3eb9799f9ea30fa786:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  397e59856f06  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  53c631458e33  000000000000  49            (missing)
  
  .hg/store/packs/manifests/61406e4caf3e020d101d44b3a0790ad31ac67e05:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  906f17f69284  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  a6875e5fbf69  000000000000  49            (missing)
  
  .hg/store/packs/manifests/7a828dc4ddbfaeff244f0e49c9a57ae4b90b8d2e:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  33600a12f793  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  40f43426c87b  000000000000  49            (missing)
  
  .hg/store/packs/manifests/eef786b4f59e9cc9a04bd2ac252a9f9b72bcca70:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  31a1621c0fb2  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  b7db2b1fa98f  000000000000  49            (missing)
  

- Clean up generated tree manifests for remaining tests.

  $ rm -rf .hg/store/packs/manifests

- Test rebasing of the flat ony commits works as expected.

  $ hg rebase -d '9055b56f3916' -s '3795bd66ca70'
  rebasing 3795bd66ca70 "flat only commit 1 over flat only commit 2 at level 2"
  fetching tree '' 40f43426c87ba597f0d9553077c72fe06d4e2acb, based on 85b359fdb09e9b8d7ac4a74551612b277345e8fd, found via 9055b56f3916
  transaction abort!
  rollback completed
  abort: "unable to find the following nodes locally or on the server: ('', 40f43426c87ba597f0d9553077c72fe06d4e2acb)"
  [255]
