#chg-compatible
  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

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
  > treemanifest=$TESTDIR/../edenscm/ext/treemanifestserver.py
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
  fetching tree '' * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  1 trees fetched over 0.00s
  fetching tree 'subdir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over 0.00s
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

  $ hg log -vpr 'desc("flat only commit 1 over flat only commit 1 at level 1")' --config treemanifest.demandgenerate=False \
  > 2>&1 > /dev/null | tail -1

- Viewing a flat draft commit in treeonly mode will generate a tree manifest
for all the commits in the path from the flat draft commit to an ancestor which
has tree manifest. In this case, this implies that tree manifest will be
generated for the commit 'b9b574be2f5d' and its parent commit '9055b56f3916'.

  $ hg log -vpr 'desc("flat only commit 1 over flat only commit 1 at level 1")' 
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       subdir/x
  description:
  flat only commit 1 over flat only commit 1 at level 1
  
  
  diff -r * -r * subdir/x (glob)
  --- a/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   x
   f1
  +f11
  
- Now that we have the tree manifest for commit 'b9b574be2f5d', we should be
able to view it even with 'treemanifest.demandgenerate' being False.

  $ hg log -vpr 'desc("flat only commit 1 over flat only commit 1 at level 1")' --config treemanifest.demandgenerate=False
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       subdir/x
  description:
  flat only commit 1 over flat only commit 1 at level 1
  
  
  diff -r * -r * subdir/x (glob)
  --- a/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   x
   f1
  +f11
  
- We should be able to also view the parent of the commit
i.e. because we now have the tree manifest for it.

  $ hg log -vpr 'desc("re:^flat only commit 1 at level 1$")' --config treemanifest.demandgenerate=False
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       subdir/x
  description:
  flat only commit 1 at level 1
  
  
  diff -r * -r * subdir/x (glob)
  --- a/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   x
  +f1
  
- Check the tree manifest for commit '9055b56f3916' and 'b9b574be2f5d'.
# TODO(meyer): Replace packfile inspection with indexedlog inspection

- Again, this would generate the tree manifest from the corresponding flat
manifest for commit.

  $ hg log -vpr 'desc("flat only commit 2 over flat only commit 1 at level 1")'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       subdir/x
  description:
  flat only commit 2 over flat only commit 1 at level 1
  
  
  diff -r * -r * subdir/x (glob)
  --- a/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   x
   f1
  +f12
  

- Tree manifest data for commit 'f7febcf0f689'.

# TODO(meyer): Replace packfile inspection with indexedlog inspection

- Clean up generated tree manifests for remaining tests.

  $ rm -rf .hg/store/manifests

- Test rebasing of the flat ony commits works as expected.

  $ hg rebase -d 'desc("re:^flat only commit 1 at level 1$")' -s 'desc("flat only commit 1 over flat only commit 2 at level 2")'
  rebasing * "flat only commit 1 over flat only commit 2 at level 2" (glob)
  fetching tree '' * (glob)
  transaction abort!
  rollback completed
  abort: "unable to find the following nodes locally or on the server: ('', *)" (glob)
  (commit: *) (glob)
  [255]
