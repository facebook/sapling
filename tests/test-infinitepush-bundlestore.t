  $ . $TESTDIR/require-ext.sh mysql

Create an ondisk bundlestore in .hg/scratchbranches

  $ extpath=`dirname $TESTDIR`
  $ cp -r $extpath/infinitepush $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > infinitepush=$TESTTMP/infinitepush
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }
  $ hg init repo
  $ cd repo
  $ ls -1 .hg/scratchbranches
  filebundlestore

Check that we can send a scratch on the server and it does not show there in
the history but is stored on disk
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > server=yes
  > EOF
  $ cd ..
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ mkcommit initialcommit
  $ hg push -r . --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ mkcommit scratchcommit
  $ hg push -r . --to scratch/mybranch --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     20759b6926ce  scratchcommit
  $ hg log -G
  @  changeset:   1:20759b6926ce
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     scratchcommit
  |
  o  changeset:   0:67145f466344
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initialcommit
  
  $ hg log -G -R ../repo
  o  changeset:   0:67145f466344
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initialcommit
  
  $ find ../repo/.hg/scratchbranches | sort
  ../repo/.hg/scratchbranches
  ../repo/.hg/scratchbranches/filebundlestore
  ../repo/.hg/scratchbranches/filebundlestore/cf
  ../repo/.hg/scratchbranches/filebundlestore/cf/b7
  ../repo/.hg/scratchbranches/filebundlestore/cf/b7/cfb730091ebd5e252fca88aab63316b11a8512b0
  ../repo/.hg/scratchbranches/index
  ../repo/.hg/scratchbranches/index/bookmarkmap
  ../repo/.hg/scratchbranches/index/bookmarkmap/scratch
  ../repo/.hg/scratchbranches/index/bookmarkmap/scratch/mybranch
  ../repo/.hg/scratchbranches/index/nodemap
  ../repo/.hg/scratchbranches/index/nodemap/20759b6926ce827d5a8c73eb1fa9726d6f7defb2

From another client we can get the scratchbranch if we ask for it explicitely

  $ cd ..
  $ hg clone ssh://user@dummy/repo client2 -q
  $ cd client2
  $ hg pull -B scratch/mybranch --traceback
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg log -G
  o  changeset:   1:20759b6926ce
  |  bookmark:    scratch/mybranch
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     scratchcommit
  |
  @  changeset:   0:67145f466344
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initialcommit
  
  $ cd ..

Push to non-scratch bookmark

  $ cd client
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newcommit
  created new head
  $ hg push -r .
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ hg log -G -T '{desc} {phase}'
  @  newcommit public
  |
  | o  scratchcommit draft
  |/
  o  initialcommit public
  

Push to scratch branch
  $ cd ../client2
  $ hg up -q scratch/mybranch
  $ mkcommit 'new scratch commit'
  $ hg push -r . --to scratch/mybranch
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 2 commits:
  remote:     20759b6926ce  scratchcommit
  remote:     1de1d7d92f89  new scratch commit

Pull scratch and non-scratch bookmark at the same time

  $ hg -R ../repo book newbook
  $ cd ../client
  $ hg pull -B newbook -B scratch/mybranch --traceback
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 2 files
  adding remote bookmark newbook
  (run 'hg update' to get a working copy)
  $ hg log -G -T '{desc} {phase}'
  o  new scratch commit draft
  |
  | @  newcommit public
  | |
  o |  scratchcommit draft
  |/
  o  initialcommit public
  
