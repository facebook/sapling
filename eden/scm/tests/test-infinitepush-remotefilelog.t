#chg-compatible

  $ setconfig extensions.treemanifest=!
  $ . "$TESTDIR/library.sh"

  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }

Create server
  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > infinitepush=
  > [remotefilelog]
  > server=True
  > [infinitepush]
  > server=True
  > branchpattern=re:scratch/.+
  > indextype=disk
  > storetype=disk
  > EOF
  $ cd ..

Create first client
  $ hgcloneshallow ssh://user@dummy/master shallow1
  streaming all changes
  0 files to transfer, 0 bytes of data
  transferred 0 bytes in * seconds (0 bytes/sec) (glob)
  no changes found
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd shallow1
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > infinitepush=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ cd ..

Create second client
  $ hgcloneshallow ssh://user@dummy/master shallow2 -q
  $ cd shallow2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > infinitepush=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ cd ..

First client: make commit and push to scratch branch
  $ cd shallow1
  $ mkcommit scratchcommit
  $ hg push -r . --to scratch/newscratch --create
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 1 commit:
  remote:     2d9cfa751213  scratchcommit
  $ cd ..

Second client: pull scratch commit and update to it
  $ cd shallow2
  $ hg pull -B scratch/newscratch
  pulling from ssh://user@dummy/master
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg up 2d9cfa751213
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

First client: make commits with file modification and file deletion
  $ cd shallow1
  $ echo 1 > 1
  $ echo 2 > 2
  $ mkdir dir
  $ echo fileindir > dir/file
  $ echo toremove > dir/toremove
  $ hg ci -Aqm 'scratch commit with many files'
  $ hg rm dir/toremove
  $ hg ci -Aqm 'scratch commit with deletion'
  $ hg push -r . --to scratch/newscratch
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 3 commits:
  remote:     2d9cfa751213  scratchcommit
  remote:     1c2153299e05  scratch commit with many files
  remote:     2db33e8c1f93  scratch commit with deletion
  $ cd ..

Second client: pull new scratch commits and update to all of them
  $ cd shallow2
  $ hg pull --config remotefilelog.excludepattern=somefile -B scratch/newscratch
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 5 changes to 5 files
  $ hg up 1c2153299e05
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up 2db33e8c1f93
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cd ..

First client: make a file whose name is a glob
  $ cd shallow1
  $ echo >> foo[bar]
  $ hg commit -Aqm "Add foo[bar]"
  $ echo >> foo[bar]
  $ hg commit -Aqm "Edit foo[bar]"
  $ hg push -r . --to scratch/regex --create
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 5 commits:
  remote:     2d9cfa751213  scratchcommit
  remote:     1c2153299e05  scratch commit with many files
  remote:     2db33e8c1f93  scratch commit with deletion
  remote:     2eea49d22494  Add foo[bar]
  remote:     fc38aa914a17  Edit foo[bar]
  $ cd ..

Second client: pull regex file an make sure it is readable
(only pull the first commit, to force a rebundle)
  $ cd shallow2
  $ hg pull -r 2eea49d22494
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 6 changes to 6 files
  $ hg log -r 2eea49d22494 --stat
  changeset:   3:2eea49d22494
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Add foo[bar]
  
   foo[bar] |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
