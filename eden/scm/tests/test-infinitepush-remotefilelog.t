#require no-eden

  $ . "$TESTDIR/library.sh"

  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }

  $ enable commitcloud
  $ disable infinitepush

Create server
  $ newserver master
  $ setconfig remotefilelog.server=true infinitepush.server=true
  $ setconfig infinitepush.branchpattern="re:scratch/.+"
  $ setconfig infinitepush.indextype=disk infinitepush.storetype=disk
  $ cd ..

Create first client
  $ hgcloneshallow test:master shallow1 -q
  $ cd shallow1
  $ setconfig infinitepush.server=false
  $ setconfig infinitepush.branchpattern="re:scratch/.+"
  $ cd ..

Create second client
  $ hgcloneshallow test:master shallow2 -q
  $ cd shallow2
  $ setconfig infinitepush.server=false
  $ setconfig infinitepush.branchpattern="re:scratch/.+"
  $ cd ..

First client: make commit and push to scratch branch
  $ cd shallow1
  $ mkcommit scratchcommit
  $ hg push -q -r . --to scratch/newscratch --create
  $ cd ..

Second client: pull scratch commit and update to it
  $ cd shallow2
  $ hg pull -q -B scratch/newscratch
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
  $ hg push -q -r . --to scratch/newscratch --force
  $ cd ..

Second client: pull new scratch commits and update to all of them
  $ cd shallow2
  $ hg pull -q --config remotefilelog.excludepattern=somefile -B scratch/newscratch
  $ hg up 70ec84a579b5
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up bae5ff92534a
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cd ..

First client: make a file whose name is a glob
  $ cd shallow1
  $ echo >> foo[bar]
  $ hg commit -Aqm "Add foo[bar]"
  $ echo >> foo[bar]
  $ hg commit -Aqm "Edit foo[bar]"
  $ hg push -q -r . --to scratch/regex --create
  $ cd ..

Second client: pull regex file an make sure it is readable
(only pull the first commit, to force a rebundle)
  $ cd shallow2
  $ hg pull -q -r 3109e6519e25
  $ hg log -r 3109e6519e25 --stat
  commit:      3109e6519e25
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Add foo[bar]
  
   foo[bar] |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
