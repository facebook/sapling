=====================
Test workflow options
=====================

  $ . "$TESTDIR/testlib/obsmarker-common.sh"

Test single head enforcing - Setup
=============================================

  $ cat << EOF >> $HGRCPATH
  > [experimental]
  > evolution = all
  > EOF
  $ hg init single-head-server
  $ cd single-head-server
  $ cat <<EOF >> .hg/hgrc
  > [phases]
  > publish = no
  > [experimental]
  > single-head-per-branch = yes
  > EOF
  $ mkcommit ROOT
  $ mkcommit c_dA0
  $ cd ..

  $ hg clone single-head-server client
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test single head enforcing - with branch only
---------------------------------------------

  $ cd client

continuing the current defaultbranch

  $ mkcommit c_dB0
  $ hg push
  pushing to $TESTTMP/single-head-server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

creating a new branch

  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg branch branch_A
  marked working directory as branch branch_A
  (branches are permanent and global, did you want a bookmark?)
  $ mkcommit c_aC0
  $ hg push --new-branch
  pushing to $TESTTMP/single-head-server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)

Create a new head on the default branch

  $ hg up 'desc("c_dA0")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit c_dD0
  created new head
  $ hg push -f
  pushing to $TESTTMP/single-head-server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  transaction abort!
  rollback completed
  abort: rejecting multiple heads on branch "default"
  (2 heads: 286d02a6e2a2 9bf953aa81f6)
  [255]

remerge them

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ mkcommit c_dE0
  $ hg push
  pushing to $TESTTMP/single-head-server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files

Test single head enforcing - after rewrite
------------------------------------------

  $ mkcommit c_dF0
  $ hg push
  pushing to $TESTTMP/single-head-server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg commit --amend -m c_dF1
  $ hg push
  pushing to $TESTTMP/single-head-server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files (+1 heads)
  1 new obsolescence markers
  obsoleted 1 changesets

Check it does to interfer with strip
------------------------------------

setup

  $ hg branch branch_A --force
  marked working directory as branch branch_A
  $ mkcommit c_aG0
  created new head
  $ hg update 'desc("c_dF1")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit c_dH0
  $ hg update 'desc("c_aG0")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ mkcommit c_aI0
  $ hg log -G
  @    changeset:   10:49003e504178
  |\   branch:      branch_A
  | |  tag:         tip
  | |  parent:      8:a33fb808fb4b
  | |  parent:      3:840af1c6bc88
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     c_aI0
  | |
  | | o  changeset:   9:fe47ea669cea
  | | |  parent:      7:99a2dc242c5d
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | | |  summary:     c_dH0
  | | |
  | o |  changeset:   8:a33fb808fb4b
  | |/   branch:      branch_A
  | |    user:        test
  | |    date:        Thu Jan 01 00:00:00 1970 +0000
  | |    summary:     c_aG0
  | |
  | o  changeset:   7:99a2dc242c5d
  | |  parent:      5:6ed1df20edb1
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     c_dF1
  | |
  | o    changeset:   5:6ed1df20edb1
  | |\   parent:      4:9bf953aa81f6
  | | |  parent:      2:286d02a6e2a2
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | | |  summary:     c_dE0
  | | |
  | | o  changeset:   4:9bf953aa81f6
  | | |  parent:      1:134bc3852ad2
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | | |  summary:     c_dD0
  | | |
  o | |  changeset:   3:840af1c6bc88
  | | |  branch:      branch_A
  | | |  parent:      0:ea207398892e
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | | |  summary:     c_aC0
  | | |
  | o |  changeset:   2:286d02a6e2a2
  | |/   user:        test
  | |    date:        Thu Jan 01 00:00:00 1970 +0000
  | |    summary:     c_dB0
  | |
  | o  changeset:   1:134bc3852ad2
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     c_dA0
  |
  o  changeset:   0:ea207398892e
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     ROOT
  

actual stripping

  $ hg strip --config extensions.strip= --rev 'desc("c_dH0")'
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/fe47ea669cea-a41bf5a9-backup.hg

