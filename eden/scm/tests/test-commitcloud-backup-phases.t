#chg-compatible

Create an ondisk bundlestore in .hg/scratchbranches
  $ setconfig extensions.treemanifest=!
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ cp $HGRCPATH $TESTTMP/defaulthgrc
  $ setupcommon
  $ hg init master
  $ cd master

Check that we can send a scratch on the server and it does not show there in
the history but is stored on disk
  $ setupserver
  $ cd ..
  $ hg clone ssh://user@dummy/master client -q
  $ cd client
  $ mkcommit "initial commit"
  $ mkcommit "another commit"
  $ hg push -r . -q
  $ mkcommit "stack 1 - commit 1"
  $ mkcommit "stack 1 - commit 2"
  $ hg up 0
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ mkcommit "stack 2 - commit 1"
  $ mkcommit "stack 2 - commit 2"
  $ hg log -G -T '{shortest(node)} {desc} {phase}'
  @  ccd5 stack 2 - commit 2 draft
  |
  o  f133 stack 2 - commit 1 draft
  |
  | o  d567 stack 1 - commit 2 draft
  | |
  | o  bc62 stack 1 - commit 1 draft
  | |
  | o  cf4b another commit public
  |/
  o  966a initial commit public
  
  $ hg cloud backup
  backing up stack rooted at bc62325caa65
  remote: pushing 2 commits:
  remote:     bc62325caa65  stack 1 - commit 1
  remote:     d567dbbdd271  stack 1 - commit 2
  backing up stack rooted at f13337e62e40
  remote: pushing 2 commits:
  remote:     f13337e62e40  stack 2 - commit 1
  remote:     ccd5ee66f08a  stack 2 - commit 2
  commitcloud: backed up 4 commits
  $ hg pull -r bc62
  pulling from ssh://user@dummy/master
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files

  $ hg log -G -T '{shortest(node)} {desc} {phase}'
  @  ccd5 stack 2 - commit 2 draft
  |
  o  f133 stack 2 - commit 1 draft
  |
  | o  d567 stack 1 - commit 2 draft
  | |
  | o  bc62 stack 1 - commit 1 draft
  | |
  | o  cf4b another commit public
  |/
  o  966a initial commit public
  
